//! An independent binary64 extended-box Gaussian (Gwosdek, Grewenig, Bruhn and
//! Weickert, "Theoretical foundations of Gaussian convolution by extended box
//! filtering", SSVM 2011), written from the paper and not from the core: for
//! `passes` = K boxes of variance σ²/K each, the box radius is
//! `l = ⌊√(12σ²/K + 1)/2 − 1/2⌋` and the end weight
//! `α = (2l + 1)(l(l + 1) − 3σ²/K) / (6(σ²/K − (l + 1)²))`. Each window sum
//! is a difference of one pass's prefix sums. Edges are exact clamp-to-edge
//! of the whole K-fold kernel: the line is padded once by its full support.
//! It also restates the fitted Gaussian Blur law (measured against
//! Photoshop) and runs it ([`fitted_blur_rgba`]).
#![allow(dead_code)]

/// (l, α) of one box of variance `v`.
fn geometry(v: f64) -> (usize, f64) {
    let l = ((12.0 * v + 1.0).sqrt() / 2.0 - 0.5).floor().max(0.0);
    let alpha = (2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * v) / (6.0 * (v - (l + 1.0).powi(2)));
    (l as usize, alpha)
}

/// One line through K extended boxes at σ, clamp-to-edge.
pub fn extended_box_line(x: &[f64], sigma: f64, passes: usize) -> Vec<f64> {
    let (l, alpha) = geometry(sigma * sigma / passes as f64);
    let support = passes * (l + 1);
    let n = x.len() as isize;
    let mut line: Vec<f64> = (-(support as isize)..n + support as isize)
        .map(|i| x[i.clamp(0, n - 1) as usize])
        .collect();
    let weight = 2.0 * l as f64 + 1.0 + 2.0 * alpha;
    for _ in 0..passes {
        let reach = l + 1;
        // prefix[i] = Σ line[..i], so the inner window is one difference.
        let mut prefix = vec![0.0; line.len() + 1];
        for (i, v) in line.iter().enumerate() {
            prefix[i + 1] = prefix[i] + v;
        }
        let next: Vec<f64> = (reach..line.len() - reach)
            .map(|i| {
                let inner = prefix[i + l + 1] - prefix[i - l];
                (inner + alpha * (line[i - reach] + line[i + reach])) / weight
            })
            .collect();
        line = next;
    }
    line
}

/// The separable filter over a w×h RGBA plane, rows then columns.
pub fn extended_box_rgba(
    src: &[[f64; 4]],
    w: usize,
    h: usize,
    sigma: f64,
    passes: usize,
) -> Vec<[f64; 4]> {
    separable(src, w, h, |line| extended_box_line(line, sigma, passes))
}

/// The Gaussian Blur law, restated from the take record
/// (`scripts/photoshop-probe/blur_wand_take.py`): from radius 7 up, 4
/// extended boxes per axis at σ = 0.99·r.
pub const FITTED_PASSES: usize = 4;
pub const FITTED_SCALE: f64 = 0.99;
/// The smallest radius the law runs; below it Gaussian Blur keeps the old kernel.
pub const FITTED_MIN_RADIUS: f64 = 7.0;

/// The fitted law's Gaussian Blur at `radius` over a premultiplied w×h plane.
pub fn fitted_blur_rgba(src: &[[f64; 4]], w: usize, h: usize, radius: f64) -> Vec<[f64; 4]> {
    extended_box_rgba(src, w, h, FITTED_SCALE * radius, FITTED_PASSES)
}

/// Rows then columns of `line` over each channel.
fn separable(
    src: &[[f64; 4]],
    w: usize,
    h: usize,
    line: impl Fn(&[f64]) -> Vec<f64>,
) -> Vec<[f64; 4]> {
    let mut rows = vec![[0.0; 4]; w * h];
    for y in 0..h {
        for c in 0..4 {
            let x: Vec<f64> = (0..w).map(|x| src[y * w + x][c]).collect();
            for (i, v) in line(&x).into_iter().enumerate() {
                rows[y * w + i][c] = v;
            }
        }
    }
    let mut out = vec![[0.0; 4]; w * h];
    for x in 0..w {
        for c in 0..4 {
            let col: Vec<f64> = (0..h).map(|y| rows[y * w + x][c]).collect();
            for (i, v) in line(&col).into_iter().enumerate() {
                out[i * w + x][c] = v;
            }
        }
    }
    out
}
