//! The radius-independent Gaussian of the legacy Gaussian Blur family:
//! 8-bit `dr_image_filter_op` `blur` / `dr_image_sharpen` through
//! `sampling::blur`, and the 16-bit `blur` / `sharpen` selected edits through
//! `selected_filter`: `blur` from σ [`LEGACY_FAST_SIGMA`] to below
//! `gaussian_blur_filter::FITTED_MIN_RADIUS` (the fitted kernel above it),
//! Sharpen at every σ ≥ [`LEGACY_FAST_SIGMA`]. Below it both keep
//! the image crate's kernel, byte for byte. The shared kernel family is a
//! caller too: `sampling_gaussian::blur_bounded`
//! (the destructive Unsharp Mask and High Pass, export sharpening and the
//! 16-bit selected Photo filters; written for the live filters removed on
//! 2026-10-10) runs it from `sampling_gaussian::LIVE_FAST_SIGMA` up, as one
//! band over the whole plane, held to a clause against an untruncated
//! Gaussian. [`band`] computes any `BAND_ROWS`-aligned row range over a ring
//! of input rows.
//!
//! The line filter is Deriche's 4th-order recursive Gaussian (INRIA RR-1893,
//! 1993): a causal and an anticausal recursion run in parallel over the same
//! input and summed. Its cost per sample is fixed whatever σ is. It was
//! chosen by measurement (`designs/phase-9f-selected-tone-status.md`, "a
//! radius-independent legacy Gaussian"), out of two extended-box filters
//! (K = 3 and K = 5), Young–van Vliet 3rd order and this one. Measured on
//! premultiplied unit-scale samples, over four fixtures at both depths:
//!
//! - Against the image crate's kernel (today's legacy output), its max error
//!   is ≤ 0.49/255 and its mean ≤ 0.07/255 at every σ ≥ 4.
//! - It is within 0.04/255 of an untruncated sampled Gaussian at σ64.
//! - Against Photoshop 26.10.0's Gaussian Blur it is as close as the exact
//!   kernel at every radius.
//!
//! Each extended box missed the legacy bound by ≥ 1.8/255 at every σ, and
//! Young–van Vliet by ≥ 1.9/255.
//!
//! Every edge is exact clamp-to-edge. A recursion's state at an end is its
//! steady state for that end sample repeated forever, which is what an
//! infinitely clamped line feeds it. The parallel form needs no
//! Triggs–Sdika correction, because both halves read the input, never each
//! other's output.
//!
//! [`band`] computes each `BAND_ROWS` tile from its own window of ±`halo`
//! rows: the image edge is exactly clamped and the window's inner edge is
//! replicated. Each tile depends only on its own window, so any aligned
//! range is bit-identical to the same rows of the whole plane. The
//! horizontal pass covers every window row. The vertical
//! pass walks rows with per-column state, never gathering a strided column.
//! Line math is binary64 and storage f32.
//!
//! From `LEGACY_FAST_SIGMA` (4) up they, and the byte `sampling::blur` behind
//! `dr_image_filter_op` `blur`/`dr_image_sharpen`, run `sampling_gaussian_fast.rs`. That is the
//! ONE radius-independent Gaussian: Deriche 4th order, chosen by measurement, parallel causal +
//! anticausal, exact clamp-to-edge. It runs each `BAND_ROWS` tile from its own ±(ceil(4σ)+4)-row
//! window, so a band is bit-identical to the whole plane, with f64 line math and f32 storage. Its
//! `plan` charges `FAST_WORK_PER_PIXEL` (56, calibrated by `tests/gaussian_radius_independent.rs`'s
//! ignored probe) per window pixel. Its measurements are in
//! `designs/phase-9f-selected-tone-status.md`.
use crate::job_control::JobContext;
use crate::precision_image::{reserved_vec, BOOKKEEPING_BYTES};
use crate::selected_filter::BAND_ROWS;
use std::ops::Range;

/// The switch: the legacy family runs this filter at σ ≥ 4 and the image
/// crate's kernel below. Measured against that kernel, the max error was
/// 1.09/255 at σ3, but ≤ 0.49/255 (mean ≤ 0.07/255) at every σ ≥ 4 up to
/// 64. The rule asked for ≤ 1/255 max and ≤ 0.1/255 mean at every σ ≥ the
/// switch. The image crate cuts its kernel at about 2.5σ below σ4, so an
/// untruncated filter cannot match it there.
pub(crate) const LEGACY_FAST_SIGMA: f32 = 4.0;

/// Work units charged per window pixel (one sample of one window row through
/// both passes), in the units of `sampling_gaussian::plan_bounded`. Its
/// calibration record follows. The `#[ignore]`d probe in
/// `tests/gaussian_radius_independent.rs` produces it.
///
/// The probe's reference is the Surface Blur probe's: byte `high_pass` r16
/// on 3000×2000 noise, charged n·(8·97+12). Against that rate it times byte
/// `dr_image_filter_op` `blur` and the 16-bit `blur` selected edit at σ ∈ {4, 12, 25,
/// 64}. It takes the largest implied units per window pixel and multiplies
/// it by 1.15, the Surface Blur probe's safety factor.
///
/// Record (2026-10-03, Apple M2 Max, release): the reference took 4.029 s,
/// 1.1734e9 units/s. Implied units per window pixel, byte / 16-bit edit:
///
/// | σ | byte | 16-bit edit |
/// |---|---|---|
/// | 4 | 27.5 | 48.4 |
/// | 12 | 25.6 | 41.8 |
/// | 25 | 23.5 | 36.0 |
/// | 64 | 22.1 | 30.6 |
///
/// The 16-bit figure includes the edit's own pass, so it overstates the
/// filter. The largest is 48.4, and 48.4 × 1.15 = 55.7, which rounds up to 56.
///
/// The shared kernel family's arm was checked against it: byte High
/// Pass at σ ∈ {4, 12, 25, 64}, converted with R16 = 1.1903e9 units/s timed
/// on the 10c8007 build in the same session (1.4% from the record above),
/// implied 34.6 / 30.1 / 26.4 / 22.8 units per window pixel (the largest of
/// three probe runs); 34.6 × 1.15 = 39.8 ≤ 56. Since then byte
/// High Pass r16 runs this filter, so the probe times byte High Pass σ3.9
/// (still the FIR) and converts with `tests/common/gaussian_reference.rs`'s
/// recorded factor.
///
/// At 6000×4000 the window rows are W = 7,204 / 10,376 / 19,656 at σ12 / 25
/// / 64. The most a coefficient can be and still fit the 24e9 one-shot limit
/// is floor(24e9 / (W·6000)) = 555 / 385 / 203, so every slider σ up to 64
/// is admitted at 24 MP.
pub(crate) const FAST_WORK_PER_PIXEL: u64 = 56;

/// The largest σ this filter accepts. `dr_image_filter_op` `blur` refuses
/// above it, and `plan` and `band` answer Err, so no σ is ever run as a
/// smaller one. The 16-bit edits stop at 64. At σ1000 the poles lie within
/// 2e-3 of 1; `tests/gaussian_radius_independent.rs` holds σ1000 to the same
/// 2-code bound against an exact Gaussian as every slider σ (an 8001-pixel
/// step and its clamped edges). Further up the 4th-order recursion's rounding
/// grows as the poles' distance from 1 to the fourth power, and at σ =
/// f32::MAX they would be exactly 1, with no DC gain left to normalize.
pub(crate) const MAX_SIGMA: f32 = 1000.0;

/// Err unless `sigma` is in (0, `MAX_SIGMA`].
fn checked(sigma: f64) -> Result<(), String> {
    if !sigma.is_finite() || sigma <= 0.0 || sigma > f64::from(MAX_SIGMA) {
        return Err("Gaussian sigma must be finite, positive and at most 1000".into());
    }
    Ok(())
}

/// Deriche's 1993 4th-order fit to exp(−x²/2σ²) at σ = 1:
/// `(a0 cos(w0 x) + a1 sin(w0 x)) e^{−b0 x} + (c0 cos(w1 x) + c1 sin(w1 x)) e^{−b1 x}`,
/// each term as (cosine weight, sine weight, decay, frequency).
const TERMS: [(f64, f64, f64, f64); 2] = [
    (1.680, 3.735, 1.783, 0.6318),
    (-0.6803, -0.2598, 1.723, 1.997),
];

/// Rows each window reaches beyond its tile; the window's inner edge is
/// replicated. The step-1 measurement ran this same windowed structure
/// (128-row tiles, ±(ceil(4σ)+4) rows, on 512-row fixtures), so its figures
/// in the module doc include the replication error.
pub(crate) fn halo(sigma: f64) -> u32 {
    ((4.0 * sigma).ceil() + 4.0) as u32
}

#[derive(Clone, Copy)]
struct Complex(f64, f64);
impl Complex {
    fn mul(self, o: Complex) -> Complex {
        Complex(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn add(self, o: Complex) -> Complex {
        Complex(self.0 + o.0, self.1 + o.1)
    }
    fn div(self, o: Complex) -> Complex {
        let d = o.0 * o.0 + o.1 * o.1;
        Complex(
            (self.0 * o.0 + self.1 * o.1) / d,
            (self.1 * o.0 - self.0 * o.1) / d,
        )
    }
}

/// The recursion: causal `y⁺[i] = Σ n_k x[i−k] − Σ d_k y⁺[i−k]`, anticausal
/// `y⁻[i] = Σ m_k x[i+k] − Σ d_k y⁻[i+k]`, normalized to unit DC gain;
/// `causal_gain` / `anticausal_gain` are each half's steady state per unit of
/// a constant input.
struct Deriche {
    n: [f64; 4],
    m: [f64; 4],
    d: [f64; 4],
    causal_gain: f64,
    anticausal_gain: f64,
}

impl Deriche {
    fn new(s: f64) -> Self {
        // The causal impulse response is Σ r_j p_j^i over four complex poles
        // (two conjugate pairs), so H⁺ = N/D with D = Π(1 − p_j z⁻¹).
        let mut poles = [Complex(0.0, 0.0); 4];
        let mut residues = [Complex(0.0, 0.0); 4];
        let mut distance = [Complex(0.0, 0.0); 4];
        for (t, &(cosine, sine, decay, frequency)) in TERMS.iter().enumerate() {
            let e = (-decay / s).exp();
            let theta = frequency / s;
            let half = (theta / 2.0).sin();
            for (k, sign) in [1.0, -1.0].into_iter().enumerate() {
                let j = 2 * t + k;
                poles[j] = Complex(e * theta.cos(), sign * e * theta.sin());
                residues[j] = Complex(cosine / 2.0, -sign * sine / 2.0);
                // 1 − p without cancellation: the DC gain divides by it, and
                // at large σ the pole is within 1e-3 of 1.
                distance[j] = Complex(
                    -(-decay / s).exp_m1() + e * 2.0 * half * half,
                    -sign * e * theta.sin(),
                );
            }
        }
        let times = |poly: &[Complex], p: Complex| {
            let mut out = vec![Complex(0.0, 0.0); poly.len() + 1];
            for (i, &c) in poly.iter().enumerate() {
                out[i] = out[i].add(c);
                out[i + 1] = out[i + 1].add(c.mul(Complex(-p.0, -p.1)));
            }
            out
        };
        let mut denominator = vec![Complex(1.0, 0.0)];
        for &p in &poles {
            denominator = times(&denominator, p);
        }
        let mut numerator = [Complex(0.0, 0.0); 4];
        let mut gain = Complex(0.0, 0.0);
        for j in 0..4 {
            let mut term = vec![residues[j]];
            for (k, &p) in poles.iter().enumerate() {
                if k != j {
                    term = times(&term, p);
                }
            }
            for i in 0..4 {
                numerator[i] = numerator[i].add(term[i]);
            }
            gain = gain.add(residues[j].div(distance[j]));
        }
        // The whole kernel is the causal half plus its mirror without the
        // shared centre tap.
        let scale = 1.0 / (2.0 * gain.0 - numerator[0].0);
        let n = numerator.map(|c| c.0 * scale);
        let d = [
            denominator[1].0,
            denominator[2].0,
            denominator[3].0,
            denominator[4].0,
        ];
        Deriche {
            n,
            m: [
                n[1] - d[0] * n[0],
                n[2] - d[1] * n[0],
                n[3] - d[2] * n[0],
                -d[3] * n[0],
            ],
            d,
            causal_gain: gain.0 * scale,
            anticausal_gain: gain.0 * scale - n[0],
        }
    }

    /// One row, in place of `y`, from `x` (`x.len() == y.len() > 0`).
    fn row(&self, x: &[[f64; 4]], y: &mut [[f64; 4]]) {
        let [n0, n1, n2, n3] = self.n;
        let [m1, m2, m3, m4] = self.m;
        let [d1, d2, d3, d4] = self.d;
        let first = x[0];
        let mut xs = [first; 3];
        let mut ys = [first.map(|v| v * self.causal_gain); 4];
        for (xi, yi) in x.iter().zip(y.iter_mut()) {
            let v: [f64; 4] = std::array::from_fn(|c| {
                n0 * xi[c] + n1 * xs[0][c] + n2 * xs[1][c] + n3 * xs[2][c]
                    - d1 * ys[0][c]
                    - d2 * ys[1][c]
                    - d3 * ys[2][c]
                    - d4 * ys[3][c]
            });
            xs = [*xi, xs[0], xs[1]];
            ys = [v, ys[0], ys[1], ys[2]];
            *yi = v;
        }
        let last = x[x.len() - 1];
        let mut xs = [last; 4];
        let mut ys = [last.map(|v| v * self.anticausal_gain); 4];
        for (xi, yi) in x.iter().zip(y.iter_mut()).rev() {
            let v: [f64; 4] = std::array::from_fn(|c| {
                m1 * xs[0][c] + m2 * xs[1][c] + m3 * xs[2][c] + m4 * xs[3][c]
                    - d1 * ys[0][c]
                    - d2 * ys[1][c]
                    - d3 * ys[2][c]
                    - d4 * ys[3][c]
            });
            xs = [*xi, xs[0], xs[1], xs[2]];
            ys = [v, ys[0], ys[1], ys[2]];
            for c in 0..4 {
                yi[c] += v[c];
            }
        }
    }
}

/// Rows of all windows a whole `h`-row plane runs, at halo `halo`.
pub(crate) fn window_rows(h: u32, halo: u32) -> u64 {
    (0..h)
        .step_by(BAND_ROWS as usize)
        .map(|t0| {
            let t1 = (t0 + BAND_ROWS).min(h);
            u64::from(t1.saturating_add(halo).min(h) - t0.saturating_sub(halo))
        })
        .sum()
}

/// (scratch bytes for `rows` output rows of any band, work units for the
/// WHOLE w×h plane). Err for a sigma outside (0, `MAX_SIGMA`]. No cap:
/// each caller applies its own.
///
/// Scratch is the f32 output rows and one f32 window, plus the binary64
/// row pair and the four-row column state, plus fourteen bookkeeping
/// allowances. Work is [`FAST_WORK_PER_PIXEL`] per window pixel over every
/// window of the plane.
///
/// Why five, not four: footprint was measured on the byte
/// arm (then `dr_image_blur`, now `dr_image_filter_op` `blur`, 6000×4000; E =
/// `/usr/bin/time -l` peak − pre-edit `ri_phys_footprint`), which no plan
/// charged then. At σ4, σ25, σ64 and σ1000 E
/// exceeded the n×4 output plus these buffers by a fixed 4.55–4.75 MB
/// (0.36–0.55 MB above four allowances), whatever the window. Five
/// allowances cover it; the 16-bit plans pay 1 MiB more for it, and the
/// byte arm is charged too (`sampling::blur_scratch_bytes`).
///
/// Why fourteen (2026-10-13): the 16-bit `blur` selected edit at σ6
/// on an opaque 6000×4000 layer (`gaussian_blur_photoshop.rs`'s
/// `measure_sixteen_bit_blur`, Release, one case per process,
/// `MallocLargeCache=0`, 3 runs and a base twin at E 0) measured E
/// 387,055,904 / 387,072,288 / 387,023,136 against a plan of 474,638,336
/// with five allowances: 1.226× the largest, under the 1.25× rule. Nine more
/// allowances (484,075,520) cover ⌈1.25 × 387,072,288⌉ = 483,840,360.
pub(crate) fn plan(dimensions: (u32, u32), sigma: f64, rows: u32) -> Result<(u64, u64), String> {
    checked(sigma)?;
    let (w, h) = dimensions;
    let w = u64::from(w);
    let halo = halo(sigma);
    let window = u64::from(h.min(BAND_ROWS.saturating_add(halo.saturating_mul(2))));
    let work = window_rows(h, halo)
        .saturating_mul(w)
        .saturating_mul(FAST_WORK_PER_PIXEL);
    let scratch =
        u64::from(rows) * w * 16 + window * w * 16 + w * (2 * 32 + 4 * 32) + 14 * BOOKKEEPING_BYTES;
    Ok((scratch, work))
}

/// Premultiplied, unit-scale rows `rows` (`rows.start % BAND_ROWS == 0`) of
/// the fast Gaussian over the plane `read` yields (premultiplied unit-scale
/// f32, row-major index). Each `BAND_ROWS` tile is computed from its own
/// window of ±halo rows, so any aligned range is bit-identical to the same
/// rows of `0..h`. Every sample is clamped to a valid premultiplied one: alpha
/// in [0, 1], colour in [0, alpha]. Progress is reported 0..1 over its own
/// rows. With `JobContext::NONE` the only `Err` is an allocation refused.
pub(crate) fn band(
    dimensions: (u32, u32),
    sigma: f64,
    rows: Range<u32>,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    checked(sigma)?;
    let (w, h) = dimensions;
    if w == 0 || !rows.start.is_multiple_of(BAND_ROWS) || rows.start >= rows.end || rows.end > h {
        return Err("fast Gaussian band rows lie outside the plane".into());
    }
    let filter = Deriche::new(sigma);
    let halo = halo(sigma);
    let width = w as usize;
    let len = (rows.end - rows.start) as usize;
    let window = h.min(BAND_ROWS.saturating_add(halo.saturating_mul(2))) as usize;
    let mut out = reserved_vec(len * width)?;
    out.resize(len * width, [0.0_f32; 4]);
    let mut win: Vec<[f32; 4]> = reserved_vec(window * width)?;
    let mut x: Vec<[f64; 4]> = reserved_vec(width)?;
    x.resize(width, [0.0; 4]);
    let mut y: Vec<[f64; 4]> = reserved_vec(width)?;
    y.resize(width, [0.0; 4]);
    let mut state: Vec<[f64; 4]> = reserved_vec(4 * width)?;
    state.resize(4 * width, [0.0; 4]);
    let [n0, n1, n2, n3] = filter.n;
    let [m1, m2, m3, m4] = filter.m;
    let [d1, d2, d3, d4] = filter.d;
    for t0 in rows.clone().step_by(BAND_ROWS as usize) {
        // The window comes from the whole tile, whatever `rows.end` is; only
        // the rows written are clipped to it.
        let tile_end = (t0 + BAND_ROWS).min(h);
        let t1 = tile_end.min(rows.end);
        let top = t0.saturating_sub(halo);
        let bottom = tile_end.saturating_add(halo).min(h);
        let n = (bottom - top) as usize;
        // Horizontal, every window row.
        win.clear();
        for r in top..bottom {
            job.check()?;
            let base = r as usize * width;
            for (i, xi) in x.iter_mut().enumerate() {
                *xi = read(base + i).map(f64::from);
            }
            filter.row(&x, &mut y);
            win.extend(y.iter().map(|v| v.map(|c| c as f32)));
        }
        let (lo, hi) = ((t0 - top) as usize, (t1 - top) as usize);
        let at = |r: usize| &win[r * width..(r + 1) * width];
        // Vertical causal half: rows top..t1, the tile's rows kept in `out`.
        let first = at(0);
        for slot in state.chunks_exact_mut(width) {
            for (s, p) in slot.iter_mut().zip(first) {
                *s = p.map(|v| f64::from(v) * filter.causal_gain);
            }
        }
        for r in 0..hi {
            job.check()?;
            let (x0, x1, x2, x3) = (
                at(r),
                at(r.saturating_sub(1)),
                at(r.saturating_sub(2)),
                at(r.saturating_sub(3)),
            );
            let (new, y1, y2, y3) = causal_slots(&mut state, width, r % 4);
            for c in 0..width {
                let y4 = new[c];
                new[c] = std::array::from_fn(|k| {
                    n0 * f64::from(x0[c][k])
                        + n1 * f64::from(x1[c][k])
                        + n2 * f64::from(x2[c][k])
                        + n3 * f64::from(x3[c][k])
                        - d1 * y1[c][k]
                        - d2 * y2[c][k]
                        - d3 * y3[c][k]
                        - d4 * y4[k]
                });
            }
            if r >= lo {
                let row = (t0 as usize - rows.start as usize + r - lo) * width;
                for (o, v) in out[row..row + width].iter_mut().zip(new.iter()) {
                    *o = v.map(|c| c as f32);
                }
            }
        }
        // Vertical anticausal half: rows bottom−1 down to t0, summed and
        // clamped into the tile.
        let last = at(n - 1);
        for slot in state.chunks_exact_mut(width) {
            for (s, p) in slot.iter_mut().zip(last) {
                *s = p.map(|v| f64::from(v) * filter.anticausal_gain);
            }
        }
        for r in (lo..n).rev() {
            job.check()?;
            let (x1, x2, x3, x4) = (
                at((r + 1).min(n - 1)),
                at((r + 2).min(n - 1)),
                at((r + 3).min(n - 1)),
                at((r + 4).min(n - 1)),
            );
            let (new, y1, y2, y3) = anticausal_slots(&mut state, width, r % 4);
            for c in 0..width {
                let y4 = new[c];
                new[c] = std::array::from_fn(|k| {
                    m1 * f64::from(x1[c][k])
                        + m2 * f64::from(x2[c][k])
                        + m3 * f64::from(x3[c][k])
                        + m4 * f64::from(x4[c][k])
                        - d1 * y1[c][k]
                        - d2 * y2[c][k]
                        - d3 * y3[c][k]
                        - d4 * y4[k]
                });
            }
            if r < hi {
                let row = (t0 as usize - rows.start as usize + r - lo) * width;
                for (o, v) in out[row..row + width].iter_mut().zip(new.iter()) {
                    let sum: [f64; 4] = std::array::from_fn(|k| f64::from(o[k]) + v[k]);
                    // A NaN fails `> 0` and `max` drops it, so it lands on 0.
                    let a = sum[3] as f32;
                    let alpha = if a > 0.0 { a.min(1.0) } else { 0.0 };
                    *o = [
                        (sum[0] as f32).max(0.0).min(alpha),
                        (sum[1] as f32).max(0.0).min(alpha),
                        (sum[2] as f32).max(0.0).min(alpha),
                        alpha,
                    ];
                }
            }
        }
        job.report(f64::from(t1 - rows.start) / len as f64, 6)?;
    }
    Ok(out)
}

type Slots<'a> = (
    &'a mut [[f64; 4]],
    &'a [[f64; 4]],
    &'a [[f64; 4]],
    &'a [[f64; 4]],
);

/// The causal ring: row r's slot is r mod 4 (holding row r − 4 until it is
/// overwritten), then rows r − 1, r − 2, r − 3.
fn causal_slots(state: &mut [[f64; 4]], width: usize, k: usize) -> Slots<'_> {
    let (a, rest) = state.split_at_mut(width);
    let (b, rest) = rest.split_at_mut(width);
    let (c, d) = rest.split_at_mut(width);
    match k {
        0 => (a, d, c, b),
        1 => (b, a, d, c),
        2 => (c, b, a, d),
        _ => (d, c, b, a),
    }
}

/// The anticausal ring: row r's slot is r mod 4 (holding row r + 4), then
/// rows r + 1, r + 2, r + 3.
fn anticausal_slots(state: &mut [[f64; 4]], width: usize, k: usize) -> Slots<'_> {
    let (a, rest) = state.split_at_mut(width);
    let (b, rest) = rest.split_at_mut(width);
    let (c, d) = rest.split_at_mut(width);
    match k {
        0 => (a, b, c, d),
        1 => (b, c, d, a),
        2 => (c, d, a, b),
        _ => (d, a, b, c),
    }
}
