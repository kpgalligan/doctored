//! The Spot Healing Brush's Type option: Content-Aware (the
//! default and the PatchMatch inpaint it always was), Proximity Match and
//! Create Texture.
//!
//! Each type is ONE synthesis step and nothing else. What every type hands
//! back is `inpaint_em::inpaint_plane`'s contract — a window-sized colour
//! plane carrying the hole AND its one-pixel outer ring, and the solve region
//! (the hole grown by one pixel) — so the shipped healing write-backs
//! (`doc_heal::heal_window_into_pixels_with_context` for bytes,
//! `heal_writeback::heal_with_selection_channels` at 16/32 bits and on CMYK
//! ink) blend every type's invention into the destination's lighting exactly
//! as they blend the inpaint's. There is no second blend, and Content-Aware
//! is the same call it always was, so its bytes cannot move.
//!
//! - **Proximity Match** copies ONE patch: the whole region translated by the
//!   offset whose neighbourhood best matches the stroke's own band of
//!   surrounding picture (`proximity_match`). A single rigid copy keeps the
//!   source's structure intact rather than re-assembling it.
//! - **Create Texture** builds a texture from the pixels UNDER the stroke
//!   (`create_texture`): it fits the low-frequency trend of the stroke's band
//!   of surroundings, keeps only the pixels under the stroke whose residual
//!   stays inside that band's own range (the blemish the stroke is there to
//!   remove does not, so it is never tiled back), tiles the kept residual's
//!   blocks over the region in a seeded order, and adds the trend back. Its
//!   texture comes only from pixels under the stroke, so a second stroke over
//!   the same area rebuilds the texture from the first stroke's result.
//!
//! [`SpotHealParams`] holds the candidate knobs the Photoshop take compares
//! (`examples/spot_heal_variants.rs`); [`SHIPPED`] is the set every caller
//! uses.

use serde::Deserialize;

use crate::doc_inpaint::Caller;
use crate::inpaint_em::{hole_distance, inpaint_plane};
use crate::inpaint_sample::Sample;
use crate::job_control::JobContext;
use crate::patchmatch::Plane;
use crate::poisson::COVERED_THRESHOLD;
use crate::rng::SplitMix64;

/// The Spot Healing Brush's Type. The wire spelling (MCP `type`, the typed
/// retouch request's `"type"`) is the snake-case name; the C code is
/// [`SpotHealType::from_code`]'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotHealType {
    /// PatchMatch inpainting from the band around the stroke.
    #[default]
    ContentAware,
    /// A texture built from the pixels under the stroke.
    CreateTexture,
    /// The best-matching neighbouring patch, copied whole.
    ProximityMatch,
}

impl SpotHealType {
    /// 0 Content-Aware, 1 Create Texture, 2 Proximity Match; `None` for any
    /// other code — a refusal, never a fallback to a type nobody asked for.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::ContentAware),
            1 => Some(Self::CreateTexture),
            2 => Some(Self::ProximityMatch),
            _ => None,
        }
    }
}

/// The knobs of the two new types. `band` is the band of surroundings in px
/// both read (Proximity Match's comparison band, Create Texture's trend and
/// admission range); `tile` is Create Texture's preferred block side and
/// `detrend` whether it removes the plane trend before tiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotHealParams {
    pub band: usize,
    pub tile: usize,
    pub detrend: bool,
}

/// The parameters every caller uses: the take's H0 (a 4 px band, 7 px
/// tiles, detrended). A 4 px band is wide enough to carry an edge's
/// direction and narrow enough that it describes the stroke's own
/// surroundings rather than the picture beyond them; a 7 px block is
/// PatchMatch's own patch side, the scale at which the rest of the inpaint
/// already treats texture as texture.
pub const SHIPPED: SpotHealParams = SpotHealParams {
    band: 4,
    tile: 7,
    detrend: true,
};

/// The most per-pixel comparisons Proximity Match's coarse offset search may
/// make: 2^27, about a tenth of a second of byte arithmetic. The search
/// stride is the smallest that keeps the offset grid times the pixels each
/// offset compares under it, so a huge stroke costs a coarser search, never
/// a longer one; the halving refinement adds at most `9·N` per halving.
const PROXIMITY_COMPARE_BUDGET: u64 = 1 << 27;

/// Proximity Match's refusal when no offset inside the window carries the
/// whole region onto clean picture.
pub(crate) const PROXIMITY_NO_PATCH: &str = "Proximity Match found no clean patch beside the stroke as large as the stroke itself; use a smaller brush, or the Content-Aware type.";

/// How far past the band's own residual range, as a share of that range, a
/// pixel under the stroke may lie and still be texture rather than blemish.
/// An eighth keeps a texture's tail that the band's few hundred samples did
/// not happen to reach, while a spot darker or brighter than everything
/// around it is left out of the pool (take1's dark disc sits 20 codes below
/// stripes 130 codes apart, past their 16-code allowance).
const TEXTURE_ADMIT_SHARE: f64 = 0.125;

/// The admission allowance on a band with no spread at all (a flat field),
/// relative to the band's largest magnitude: f64 round-off of the fitted
/// plane, not a tolerance, so a flat field admits only itself at any depth.
const TEXTURE_ROUNDOFF: f64 = 1e-9;

/// Create Texture's refusal when the stroke covers no pixel with colour.
pub(crate) const TEXTURE_NOTHING: &str = "Create Texture has nothing to build a texture from: the pixels under the stroke are transparent in the sampled picture.";

/// One component's synthesis by `kind`, with `inpaint_plane`'s return
/// contract: the colour plane (the hole and its one-pixel outer ring
/// written) and the solve region (the hole grown by one pixel, a coverage
/// byte per window pixel).
///
/// `solid` marks the window pixels whose sampled colour is real (alpha at or
/// over the half rule); only Create Texture reads it, and `None` there means
/// every hole pixel is solid. `ring`, `seed`, `fine_sdf`, `caller` and `lift`
/// are `inpaint_plane`'s; Proximity Match reads the ring (its search radius)
/// and the distance field, Create Texture the distance field and the seed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize<S: Sample, const C: usize>(
    kind: SpotHealType,
    params: SpotHealParams,
    plane: Plane<S, C>,
    solid: Option<&[bool]>,
    ring: f32,
    seed: u64,
    fine_sdf: Option<Vec<f32>>,
    caller: Caller,
    lift: u32,
    control: JobContext<'_>,
) -> Result<(Vec<S>, Vec<u8>), String> {
    match kind {
        SpotHealType::ContentAware => {
            inpaint_plane(plane, ring, seed, fine_sdf, caller, lift, control)
        }
        SpotHealType::ProximityMatch => proximity_match(&plane, params, ring, fine_sdf, control),
        SpotHealType::CreateTexture => {
            create_texture(&plane, params, solid, seed, fine_sdf, control)
        }
    }
}

/// The hole grown by one pixel — `inpaint_plane`'s solve region, from the
/// same contour offset, so every type hands the write-back the same shape.
fn grown_region<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    control: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let mut region: Vec<u8> = plane
        .hole
        .iter()
        .map(|c| if *c { 255 } else { 0 })
        .collect();
    crate::doc_select::offset_contour_with_context(&mut region, plane.w, plane.h, 1.0, control)?;
    Ok(region)
}

/// The window coordinates of every pixel `keep` accepts.
fn coordinates(w: usize, n: usize, keep: impl Fn(usize) -> bool) -> Vec<(i64, i64)> {
    (0..n)
        .filter(|i| keep(*i))
        .map(|i| ((i % w) as i64, (i / w) as i64))
        .collect()
}

/// One scored offset: `(score, dx² + dy², dy, dx)`, compared in that order —
/// the lowest score, then the nearer offset, then the higher, then the
/// further left, so a tie between equally good patches always lands on the
/// same one whatever order the search visited them in.
type Scored = (f64, i64, i64, i64);

fn better(candidate: Scored, best: Option<Scored>) -> bool {
    match best {
        None => true,
        Some(best) => candidate
            .partial_cmp(&best)
            .is_some_and(|o| o == std::cmp::Ordering::Less),
    }
}

/// Proximity Match: the region copied whole from the offset `d` whose band
/// matches best.
///
/// The band `B` is every usable pixel outside the hole within `params.band`
/// px of it. An offset is a candidate when it carries EVERY region pixel onto
/// a usable pixel of the window (real colour, not any hole, eligible) — a
/// patch that lands partly on another blemish or off the window is not a
/// patch — and carries at least half of `B` onto usable pixels too, so the
/// score compares a real share of the surroundings. The score is the mean
/// squared colour distance over the band pixels that land; the lowest wins.
///
/// The search covers `[-R, R]²` with `R = ⌈ring⌉`, coarse at the stride
/// [`PROXIMITY_COMPARE_BUDGET`] allows, then refined by halving the stride
/// around the best offset (a 3 × 3 neighbourhood at each), which recovers the
/// exact offset of a smooth score basin without paying for every offset.
fn proximity_match<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    params: SpotHealParams,
    ring: f32,
    fine_sdf: Option<Vec<f32>>,
    control: JobContext<'_>,
) -> Result<(Vec<S>, Vec<u8>), String> {
    control.check()?;
    let (w, h) = (plane.w, plane.h);
    let n = w * h;
    let region = grown_region(plane, control)?;
    let sdf = match fine_sdf {
        Some(sdf) => sdf,
        None => hole_distance(&plane.hole, w, h, control)?,
    };
    let usable = |i: usize| plane.data[i] && plane.eligibility.as_ref().is_none_or(|e| e[i]);
    let band = params.band as f32;
    let targets = coordinates(w, n, |i| region[i] >= COVERED_THRESHOLD);
    // Outside pixels have a negative distance; the nearest are about -0.5.
    let boundary = coordinates(w, n, |i| {
        !plane.hole[i] && sdf[i] < 0.0 && -sdf[i] <= band && usable(i)
    });
    let at = |x: i64, y: i64| -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h)
            .then(|| y as usize * w + x as usize)
            .filter(|q| usable(*q))
    };
    let pixel = |i: usize| &plane.rgb[i * C..i * C + C];
    let evaluate = |dx: i64, dy: i64| -> Option<Scored> {
        for &(x, y) in &targets {
            at(x + dx, y + dy)?;
        }
        let mut sum = 0f64;
        let mut landed = 0usize;
        for &(x, y) in &boundary {
            if let Some(q) = at(x + dx, y + dy) {
                sum += S::squared_distance(pixel(y as usize * w + x as usize), pixel(q));
                landed += 1;
            }
        }
        if landed * 2 < boundary.len() {
            return None;
        }
        let score = if landed == 0 {
            0.0
        } else {
            sum / landed as f64
        };
        Some((score, dx * dx + dy * dy, dy, dx))
    };

    let radius = ring.max(0.0).ceil() as i64;
    let compares = (targets.len() + boundary.len()).max(1) as u64;
    // The smallest stride whose grid fits the budget; at `2R + 1` the grid is
    // one offset, so the search always ends.
    let stride = (1..=2 * radius + 1)
        .find(|s| {
            let side = (2 * radius / s + 1) as u64;
            side.saturating_mul(side).saturating_mul(compares) <= PROXIMITY_COMPARE_BUDGET
        })
        .unwrap_or(2 * radius + 1);
    let side = 2 * radius / stride + 1;
    let mut best: Option<Scored> = None;
    let mut index = 0usize;
    for ky in 0..side {
        for kx in 0..side {
            control.every(index)?;
            index += 1;
            let (dx, dy) = (-radius + kx * stride, -radius + ky * stride);
            if let Some(scored) = evaluate(dx, dy) {
                if better(scored, best) {
                    best = Some(scored);
                }
            }
        }
    }
    let Some(mut best) = best else {
        return Err(PROXIMITY_NO_PATCH.to_string());
    };
    let mut step = stride;
    while step > 1 {
        step = (step + 1) / 2;
        let (bx, by) = (best.3, best.2);
        for oy in -1..=1 {
            for ox in -1..=1 {
                control.every(index)?;
                index += 1;
                if let Some(scored) = evaluate(bx + ox * step, by + oy * step) {
                    if better(scored, Some(best)) {
                        best = scored;
                    }
                }
            }
        }
    }

    let (dx, dy) = (best.3, best.2);
    let mut source = plane.rgb.clone();
    for (k, &(x, y)) in targets.iter().enumerate() {
        control.every(k)?;
        let p = y as usize * w + x as usize;
        let q = (y + dy) as usize * w + (x + dx) as usize;
        source[p * C..p * C + C].copy_from_slice(&plane.rgb[q * C..q * C + C]);
    }
    Ok((source, region))
}

/// A per-channel least-squares plane `mean + b·(x − mx) + c·(y − my)` over
/// the reference (the band, else the stroke's solid pixels), or all zeros
/// when the texture is tiled raw.
///
/// Centred coordinates keep the normal equations well conditioned on a
/// window thousands of pixels wide; a pool on one row or one column (or
/// along one diagonal) has no second direction to fit, and takes its mean.
struct Trend<const C: usize> {
    mean: [f64; C],
    slope_x: [f64; C],
    slope_y: [f64; C],
    centre: (f64, f64),
}

impl<const C: usize> Trend<C> {
    fn zero() -> Self {
        Trend {
            mean: [0.0; C],
            slope_x: [0.0; C],
            slope_y: [0.0; C],
            centre: (0.0, 0.0),
        }
    }

    fn fit<S: Sample>(plane: &Plane<S, C>, pool: &[(i64, i64)]) -> Self {
        let count = pool.len().max(1) as f64;
        let w = plane.w;
        let value = |x: i64, y: i64, c: usize| {
            f64::from(plane.rgb[(y as usize * w + x as usize) * C + c].number())
        };
        let (mut mx, mut my) = (0f64, 0f64);
        let mut mean = [0f64; C];
        for &(x, y) in pool {
            mx += x as f64;
            my += y as f64;
            for (c, m) in mean.iter_mut().enumerate() {
                *m += value(x, y, c);
            }
        }
        mx /= count;
        my /= count;
        for m in &mut mean {
            *m /= count;
        }
        let (mut sxx, mut syy, mut sxy) = (0f64, 0f64, 0f64);
        let mut sxv = [0f64; C];
        let mut syv = [0f64; C];
        for &(x, y) in pool {
            let (u, v) = (x as f64 - mx, y as f64 - my);
            sxx += u * u;
            syy += v * v;
            sxy += u * v;
            for c in 0..C {
                let d = value(x, y, c) - mean[c];
                sxv[c] += u * d;
                syv[c] += v * d;
            }
        }
        let det = sxx * syy - sxy * sxy;
        let mut trend = Trend {
            mean,
            slope_x: [0.0; C],
            slope_y: [0.0; C],
            centre: (mx, my),
        };
        // Relative to the two variances, so the test does not depend on how
        // large the pool is; a collinear pool's determinant is round-off.
        if det > 1e-9 * sxx * syy && det > 0.0 {
            for c in 0..C {
                trend.slope_x[c] = (syy * sxv[c] - sxy * syv[c]) / det;
                trend.slope_y[c] = (sxx * syv[c] - sxy * sxv[c]) / det;
            }
        }
        trend
    }

    fn at(&self, c: usize, x: usize, y: usize) -> f64 {
        self.mean[c]
            + self.slope_x[c] * (x as f64 - self.centre.0)
            + self.slope_y[c] * (y as f64 - self.centre.1)
    }
}

/// Create Texture: the region tiled with blocks of the pool's own residual.
///
/// The REFERENCE is the band of usable pixels outside the hole within
/// `params.band` px of it (Proximity Match's band); with none, the solid
/// hole pixels stand in for it. The plane trend is fitted on the reference
/// ([`Trend`]) — the surroundings say what the healed area's low frequencies
/// should be, where the hole's own trend would carry the blemish's darkness.
/// With `detrend` the residual is taken against that trend, so a block cut
/// from the bright end of a gradient does not paint a bright patch at the
/// dark end; without it the raw values are tiled.
///
/// The POOL is the hole's solid pixels whose residual lies, on every
/// channel, inside the reference's own residual range widened by
/// [`TEXTURE_ADMIT_SHARE`] of it: texture like the surroundings' is kept,
/// and a blemish that departs from everything around it is not, so the
/// stroke never tiles back the spot it is removing. The block side is the
/// largest of `params.tile`, 5 and 3 (none above `params.tile`) that fits at
/// least two whole blocks in the pool — one block would tile a single
/// repeated picture — and 1 when none does. A window-anchored grid of that
/// side covers the region, and each grid tile draws its block from the
/// positional generator, so the texture is reproducible from the seed and
/// independent of the order tiles are written in. A pool with no pixel at
/// all leaves the region the fitted trend alone: there is nothing under the
/// stroke that is texture rather than blemish.
fn create_texture<S: Sample, const C: usize>(
    plane: &Plane<S, C>,
    params: SpotHealParams,
    solid: Option<&[bool]>,
    seed: u64,
    fine_sdf: Option<Vec<f32>>,
    control: JobContext<'_>,
) -> Result<(Vec<S>, Vec<u8>), String> {
    control.check()?;
    let (w, h) = (plane.w, plane.h);
    let n = w * h;
    let solid_hole = |i: usize| plane.hole[i] && solid.is_none_or(|s| s[i]);
    let under = coordinates(w, n, solid_hole);
    if under.is_empty() {
        return Err(TEXTURE_NOTHING.to_string());
    }
    let region = grown_region(plane, control)?;
    let sdf = match fine_sdf {
        Some(sdf) => sdf,
        None => hole_distance(&plane.hole, w, h, control)?,
    };
    let usable = |i: usize| plane.data[i] && plane.eligibility.as_ref().is_none_or(|e| e[i]);
    let band = params.band as f32;
    // Outside pixels have a negative distance; the nearest are about -0.5.
    let surround = coordinates(w, n, |i| {
        !plane.hole[i] && sdf[i] < 0.0 && -sdf[i] <= band && usable(i)
    });
    let reference = if surround.is_empty() {
        &under
    } else {
        &surround
    };
    let fit = Trend::fit(plane, reference);
    let zero = Trend::zero();
    let base = if params.detrend { &fit } else { &zero };
    let residual =
        |i: usize, c: usize| f64::from(plane.rgb[i * C + c].number()) - base.at(c, i % w, i / w);
    let (mut low, mut high) = ([f64::INFINITY; C], [f64::NEG_INFINITY; C]);
    let mut magnitude = 0f64;
    for &(x, y) in reference {
        let i = y as usize * w + x as usize;
        for c in 0..C {
            let r = residual(i, c);
            low[c] = low[c].min(r);
            high[c] = high[c].max(r);
            magnitude = magnitude.max(f64::from(plane.rgb[i * C + c].number()).abs());
        }
    }
    let allowance: [f64; C] = std::array::from_fn(|c| {
        (high[c] - low[c]) * TEXTURE_ADMIT_SHARE + magnitude * TEXTURE_ROUNDOFF
    });
    let mut admitted = vec![false; n];
    for &(x, y) in &under {
        let i = y as usize * w + x as usize;
        admitted[i] = (0..C).all(|c| {
            let r = residual(i, c);
            r >= low[c] - allowance[c] && r <= high[c] + allowance[c]
        });
    }
    // A summed-area table of the pool, so whether a block lies wholly in it
    // is four reads whatever its side.
    let mut summed = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        control.check()?;
        let mut row = 0u32;
        for x in 0..w {
            row += u32::from(admitted[y * w + x]);
            summed[(y + 1) * (w + 1) + x + 1] = summed[y * (w + 1) + x + 1] + row;
        }
    }
    let whole = |x: usize, y: usize, t: usize| {
        let s = |xx: usize, yy: usize| summed[yy * (w + 1) + xx];
        s(x + t, y + t) + s(x, y) - s(x + t, y) - s(x, y + t) == (t * t) as u32
    };
    let blocks_of = |t: usize| -> Result<Vec<(usize, usize)>, String> {
        let mut blocks = Vec::new();
        if t > w || t > h {
            return Ok(blocks);
        }
        for y in 0..=h - t {
            control.check()?;
            for x in 0..=w - t {
                if whole(x, y, t) {
                    blocks.push((x, y));
                }
            }
        }
        Ok(blocks)
    };
    let mut ladder: Vec<usize> = [params.tile.max(1), 5, 3]
        .into_iter()
        .filter(|t| *t <= params.tile.max(1))
        .collect();
    ladder.sort_unstable_by(|a, b| b.cmp(a));
    ladder.dedup();
    let mut chosen = None;
    for t in ladder {
        let blocks = blocks_of(t)?;
        if blocks.len() >= 2 {
            chosen = Some((t, blocks));
            break;
        }
    }
    let (t, blocks) = match chosen {
        Some(chosen) => chosen,
        None => (
            1,
            (0..n)
                .filter(|i| admitted[*i])
                .map(|i| (i % w, i / w))
                .collect(),
        ),
    };

    let mut source = plane.rgb.clone();
    for (i, r) in region.iter().enumerate() {
        control.every(i)?;
        if *r < COVERED_THRESHOLD {
            continue;
        }
        let (x, y) = (i % w, i / w);
        if blocks.is_empty() {
            for c in 0..C {
                source[i * C + c] = S::filtered(fit.at(c, x, y) as f32);
            }
            continue;
        }
        let (tx, ty) = (x / t, y / t);
        let k = SplitMix64::at(seed, tx as u64, ty as u64, t as u64).below(blocks.len());
        let (bx, by) = blocks[k];
        let (qx, qy) = (bx + x - tx * t, by + y - ty * t);
        let q = qy * w + qx;
        for c in 0..C {
            source[i * C + c] = S::filtered((base.at(c, x, y) + residual(q, c)) as f32);
        }
    }
    Ok((source, region))
}
