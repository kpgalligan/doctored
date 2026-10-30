//! `curves`: the per-channel point lists of an adjustment, resolved into
//! 256-entry lookup tables at parse time. The payload, its `parse` and its
//! kernel live here; `adjust` keeps only the schema row and the dispatch.
//!
//! Each supplied point list is sorted by `in` (a stable sort, so of two
//! points sharing an `in` value the first listed wins and later duplicates
//! are dropped; fewer than 2 distinct `in` values is malformed) and turned
//! into a NATURAL cubic spline through the points ([`Spline`]:
//! `S'' = 0` at both ends, the second derivatives from the tridiagonal
//! system). It passes through every control point exactly and, unlike a
//! monotone interpolant, may overshoot between them.
//! It is the ONE spline: the 8-bit table samples it at each code, the
//! high-depth evaluation (`Continuous::Points`) at the actual input, and the
//! 32-bit float form ([`CurveLuts::apply_float`]) with its measured end rules.
//! Fitted to Photoshop 26.10.0 (take2, every curves set on both
//! profiles): its 16-bit render within 4.4e-5 and its 32-bit render within
//! 1.9e-7, where the Fritsch–Carlson monotone cubic used before missed by
//! 0.214 at 32 bits; Fritsch–Carlson is retired. On take2's S-curve the
//! change moves the curve by up to 0.0056 (1.4 codes; 2 codes in the rounded
//! 8-bit table), more on steep curves; a two-point curve (a straight line) is
//! unchanged.
//! At 8 and 16 bits, inputs outside the listed `in` range take the nearest
//! endpoint's `out` value and the result is clamped to [0, 255]. The
//! identity point list `[[0, 0], [255, 255]]` produces the identity table.
//! Application order per channel: the per-channel table first, then the
//! master `rgb` table — `out = rgb_lut[channel_lut[v]]`.
//!
//! AT 32 BITS ([`Spline::value_float`]) nothing is clamped: inside the
//! points the curve is `S(X)` with `X = 255·v`; above the last point it
//! continues along the end slope, `yn + S'(xn)·(X − xn)`; below the first it
//! stays flat at `y0`, except that a first point at `x = 0` continues along
//! its slope, `y0 + S'(0)·X`. The channel curve runs before the master
//! curve. The map form below takes [`map_value_float`] at 32 bits:
//! piecewise LINEAR through its 256 entries on `X = 255·v`, its first and
//! last segments continued along their own slopes beyond 0…255 (`m0 +
//! X·(m1 − m0)` below, `m255 + (X − 255)·(m255 − m254)` above), nothing
//! clamped, the channel map before the master. Measured in Photoshop 26.10.0
//! (receipt `float-adjust-473/take3`, runner `float_adjust_take3.py` sha
//! `ff7d93d3…`): CM-in `lin` 0.0012, CM-order `channel-first` 0.98, CM-hi
//! `slope` 0.0076 and CM-lo `slope` 5.6e-5 tolerance units (T = 2e-3·max(1,
//! |want|)), its destructive apply equal to the layer.
//!
//! MAP FORM (Photoshop's pencil-drawn `curv` with `is_map` 1):
//! `rgb_map`, `r_map`, `g_map`, `b_map` each hold exactly 256 integers
//! 0…255, entry i being the output for input code i. They are kept VERBATIM
//! rather than fitted to points, because a drawn map is freely non-monotone
//! (a notch, a spike) and no ≤16-point cubic can hold it. Any map
//! key makes the whole adjustment map form — a point key beside a map key
//! is malformed, matching `curv`'s single `is_map` flag — and a missing
//! channel is the identity map. The order is the same as for points: the
//! channel map, then `rgb_map`. At 8 bits the maps ARE the lookup tables,
//! so `out = rgb_map[channel_map[v]]` exactly. At 16 bits Photoshop reads
//! them through all-integer 15-bit STAGE TABLES,
//! with `R(n, d) = floor((2n + d)/(2d))` (`adjust_color_balance::round_ratio`):
//! each entry becomes `q_i = R(32768·m_i, 255)`; the stage table [`stage15`]
//! has 4097 nodes, `S[j] = q_i + R((q_{i+1} − q_i)(255j − 4096i), 4096)` with
//! `i = min(⌊255j/4096⌋, 254)`; [`read15`] reads a table at a 15-bit code `x`
//! as `A[i] + R((A[i+1] − A[i])(x − 8i), 8)`, `i = min(⌊x/8⌋, 4095)`; each
//! channel composes ONE table `T_c[j] = read(S_master, S_channel[j])` at parse
//! time; a stored code `u` reads `T_c` at `k = R(32768·u, 65535)` and writes
//! `R(65535·read, 32768)`. Measured in Photoshop 26.10.0 (receipt
//! `float-adjust-473/take5`, runner `float_adjust_take5.py` sha `3d2ac9fb…`,
//! CM16X): exact to the 16-bit code on every sample of six map sets, where
//! the continuous readings miss there (`lin`, piecewise-linear on the 15-bit
//! code, by up to 58887 codes; take4's `stage4096/compose` by 2040). 32 bits keep
//! [`map_value_float`] (above).
//!
//! point lists (`rgb`, `r`, `g`, `b`; the natural spline, `Spline` at high depth) OR the MAP
//! form a Photoshop pencil curve (`curv` `is_map` 1) imports as —
//! `rgb_map`/`r_map`/`g_map`/`b_map`, each exactly 256 integers 0…255 kept verbatim (never
//! clamped; a point key beside a map key is malformed). The
//! channel's table applies before the composite's: exactly `rgb_map[ch_map[v]]` at 8 bits,
//! `Continuous::Maps`' three composed 15-bit stage tables at 16 bits (`stage15`/`read15`)
//! and the f64 piecewise-linear `map_value_float` at 32-bit float. `psd_adjustment`
//! decodes every set bit's 256 bytes and `curve_map_body` writes the body plus Photoshop's `Crv `
//! v3 tail, byte-identical to Photoshop's own. Tests: `tests/adjust_curves_map.rs`,
//! `tests/curves_native.rs`, `tests/psd_curves_map.rs` (the committed `curves_map/` Photoshop take,
//! exact at 16 bits) and `tests/psd_photoshop_channels.rs`'s `curv_map` gate.

use serde_json::{Map, Value};
use std::sync::Arc;

use crate::adjust_color_balance::round_ratio;

/// The point-form channel keys, composite first (the `curv` bit order).
pub(crate) const POINT_KEYS: [&str; 4] = ["rgb", "r", "g", "b"];
/// The map-form channel keys, in the same order as [`POINT_KEYS`].
pub(crate) const MAP_KEYS: [&str; 4] = ["rgb_map", "r_map", "g_map", "b_map"];

/// The 256-entry per-channel lookup tables of a parsed `curves` op, built
/// once at parse time (missing channels are the identity table).
#[derive(Clone)]
pub(crate) struct CurveLuts {
    r: [u8; 256],
    g: [u8; 256],
    b: [u8; 256],
    rgb: [u8; 256],
    precision: Option<Continuous>,
}

/// The high-depth evaluation a typed parse selects: the point splines, or
/// the map form's three composed 15-bit tables `T_r`, `T_g`, `T_b` (module
/// doc), built once at parse time.
#[derive(Clone)]
enum Continuous {
    Points(Arc<[Spline; 4]>),
    Maps(Arc<[[u16; STAGE_NODES + 1]; 3]>),
}

/// Intervals of Photoshop's 15-bit stage table (module doc): 4097
/// nodes over 0…32768, eight 15-bit codes per interval.
pub(crate) const STAGE_NODES: usize = 4096;

impl CurveLuts {
    /// Parses the `curves` params per the schema row in `adjust`.
    pub(crate) fn parse(params: &Map<String, Value>) -> Option<CurveLuts> {
        let mut luts = CurveLuts {
            r: identity_lut(),
            g: identity_lut(),
            b: identity_lut(),
            rgb: identity_lut(),
            precision: None,
        };
        let maps = MAP_KEYS.iter().any(|key| params.contains_key(*key));
        if maps && POINT_KEYS.iter().any(|key| params.contains_key(*key)) {
            return None;
        }
        let keys = if maps { MAP_KEYS } else { POINT_KEYS };
        for (key, lut) in
            keys.into_iter()
                .zip([&mut luts.rgb, &mut luts.r, &mut luts.g, &mut luts.b])
        {
            if let Some(value) = params.get(key) {
                *lut = if maps {
                    parse_map(value)?
                } else {
                    spline_lut(&parse_points(value)?)
                };
            }
        }
        Some(luts)
    }

    pub(crate) fn parse_precision(params: &Map<String, Value>) -> Option<Self> {
        let mut result = Self::parse(params)?;
        if MAP_KEYS.iter().any(|key| params.contains_key(*key)) {
            let tables = [&result.r, &result.g, &result.b]
                .map(|channel| composed_map_table(&result.rgb, channel));
            result.precision = Some(Continuous::Maps(Arc::new(tables)));
            return Some(result);
        }
        let mut channels = Vec::new();
        for key in ["r", "g", "b", "rgb"] {
            let points = match params.get(key) {
                Some(value) => parse_points(value)?,
                None => vec![(0.0, 0.0), (255.0, 255.0)],
            };
            let spline = Spline::new(&points);
            if !spline.m.iter().all(|value| value.is_finite()) {
                return None;
            }
            channels.push(spline);
        }
        result.precision = Some(Continuous::Points(Arc::new(channels.try_into().ok()?)));
        Some(result)
    }
    pub(crate) fn is_identity(&self) -> bool {
        match &self.precision {
            None => false,
            Some(Continuous::Maps(_)) => [&self.rgb, &self.r, &self.g, &self.b]
                .iter()
                .all(|table| **table == identity_lut()),
            Some(Continuous::Points(channels)) => channels.iter().all(|s| {
                s.points.first() == Some(&(0.0, 0.0))
                    && s.points.last() == Some(&(255.0, 255.0))
                    && s.points.iter().all(|(x, y)| x == y)
            }),
        }
    }
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        match &self.precision {
            Some(Continuous::Points(channels)) => {
                inventory.object(&**channels, "precision_curves", false);
                for spline in &**channels {
                    inventory.vec(&spline.points, "precision_curves", false);
                    inventory.vec(&spline.m, "precision_curves", false);
                }
            }
            Some(Continuous::Maps(tables)) => {
                inventory.object(&**tables, "precision_curves", false);
            }
            None => {}
        }
    }
    /// The 32-bit curve on stored linear values (module doc): each
    /// channel's [`Spline::value_float`] in 0–255 units, then the master's,
    /// with no clamp; the map form the same way through [`map_value_float`].
    /// `None` for a parse that kept no continuous form.
    pub(crate) fn apply_float(&self, rgb: [f32; 3]) -> Option<[f32; 3]> {
        match &self.precision {
            Some(Continuous::Points(channels)) => Some(std::array::from_fn(|c| {
                let channel = channels[c].value_float(f64::from(rgb[c]) * 255.0);
                (channels[3].value_float(channel) / 255.0) as f32
            })),
            Some(Continuous::Maps(_)) => {
                let tables = [&self.r, &self.g, &self.b];
                Some(std::array::from_fn(|c| {
                    let channel = map_value_float(tables[c], f64::from(rgb[c]) * 255.0);
                    (map_value_float(&self.rgb, channel) / 255.0) as f32
                }))
            }
            None => None,
        }
    }
    /// One straight RGB triple in [0, 1]: quantize to a table index, the
    /// per-channel table first, then the master `rgb` table.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        match &self.precision {
            Some(Continuous::Points(channels)) => {
                return std::array::from_fn(|c| {
                    (channels[3].value(channels[c].value(f64::from(rgb[c]) * 255.0)) / 255.0) as f32
                });
            }
            Some(Continuous::Maps(tables)) => {
                return std::array::from_fn(|c| read_map_table(&tables[c], rgb[c]));
            }
            None => {}
        }
        let idx = rgb.map(|v| (v * 255.0).round() as usize);
        [
            f32::from(self.rgb[usize::from(self.r[idx[0]])]) / 255.0,
            f32::from(self.rgb[usize::from(self.g[idx[1]])]) / 255.0,
            f32::from(self.rgb[usize::from(self.b[idx[2]])]) / 255.0,
        ]
    }
}

/// Parses one curves point list: 2..=16 `[in, out]` pairs, values clamped to
/// [0, 255], sorted by `in` with duplicate `in`s dropped (first listed wins).
/// `None` on any structural violation or fewer than 2 distinct `in` values.
pub(crate) fn parse_points(value: &Value) -> Option<Vec<(f64, f64)>> {
    let list = value.as_array()?;
    if !(2..=16).contains(&list.len()) {
        return None;
    }
    let mut points = Vec::with_capacity(list.len());
    for entry in list {
        let pair = entry.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let x = pair[0].as_f64()?.clamp(0.0, 255.0);
        let y = pair[1].as_f64()?.clamp(0.0, 255.0);
        points.push((x, y));
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.dedup_by(|next, kept| next.0 == kept.0);
    if points.len() < 2 {
        return None;
    }
    Some(points)
}

/// Parses one map-form channel: exactly 256 numbers, each an integer
/// 0…255 (a float with a zero fraction is accepted). Anything else is
/// `None` — a map is never clamped, because a value outside it is not a
/// drawn curve but a corrupt one.
pub(crate) fn parse_map(value: &Value) -> Option<[u8; 256]> {
    let list = value.as_array()?;
    if list.len() != 256 {
        return None;
    }
    let mut table = [0u8; 256];
    for (slot, entry) in table.iter_mut().zip(list) {
        let v = entry.as_f64()?;
        if v.fract() != 0.0 || !(0.0..=255.0).contains(&v) {
            return None;
        }
        *slot = v as u8;
    }
    Some(table)
}

/// `S_m`, one map's 15-bit stage table (module doc): each entry
/// `q_i = R(32768·m_i, 255)`, then node `j` of 0…4096 the straight line
/// between the two entries bracketing `255·j/4096`, rounded half up in
/// integers, so the identity map gives `S[j] = 8·j` exactly.
fn stage15(map: &[u8; 256]) -> [u16; STAGE_NODES + 1] {
    let q: [i64; 256] = std::array::from_fn(|i| round_ratio(32768 * i64::from(map[i]), 255));
    let nodes = STAGE_NODES as i64;
    std::array::from_fn(|j| {
        let j = j as i64;
        let i = (255 * j / nodes).min(254);
        let i_at = i as usize;
        let step = (q[i_at + 1] - q[i_at]) * (255 * j - nodes * i);
        (q[i_at] + round_ratio(step, nodes)) as u16
    })
}

/// `T_c[j] = read(S_master, S_channel[j])` (module doc): one channel's
/// composed 15-bit table, the master map applied after the channel's. The
/// 16-bit map reading of RGB curves and of CMYK ink curves
/// (`cmyk_adjust::CmykAdjustment::CurveMaps`).
pub(crate) fn composed_map_table(
    master: &[u8; 256],
    channel: &[u8; 256],
) -> [u16; STAGE_NODES + 1] {
    let master = stage15(master);
    let stage = stage15(channel);
    std::array::from_fn(|j| read15(&master, u32::from(stage[j])))
}

/// A [0, 1] value through a [`composed_map_table`] at its 16-bit code (module
/// doc): `k = R(32768·u, 65535)`, read, written back as `R(65535·read, 32768)`.
pub(crate) fn read_map_table(table: &[u16; STAGE_NODES + 1], v: f32) -> f32 {
    unit16(read15(table, code15(v)))
}

/// `read(A, x)` (module doc): the 15-bit code `x`, clamped to 0…32768, read
/// on a 4097-node table as the straight line between the two nodes eight
/// codes apart that bracket it, rounded half up in integers.
fn read15(table: &[u16; STAGE_NODES + 1], code: u32) -> u16 {
    let x = i64::from(code.min(32768));
    let i = (x / 8).min(STAGE_NODES as i64 - 1);
    let low = i64::from(table[i as usize]);
    let high = i64::from(table[i as usize + 1]);
    (low + round_ratio((high - low) * (x - 8 * i), 8)) as u16
}

/// The 15-bit code `k = R(32768·u, 65535)` of a [0, 1] value through its
/// nearest 16-bit code `u` (module doc), which is what a 16-bit document
/// stores; out-of-range and NaN inputs clamp first.
fn code15(v: f32) -> u32 {
    let u = (v.clamp(0.0, 1.0) * 65535.0).round();
    let u = if u.is_finite() { u as i64 } else { 0 };
    round_ratio(32768 * u, 65535) as u32
}

/// A 15-bit result as the [0, 1] value of its 16-bit code
/// `R(65535·code, 32768)`, which the u16 writer stores back exactly.
fn unit16(code: u16) -> f32 {
    round_ratio(65535 * i64::from(code), 32768) as f32 / 65535.0
}

/// The 32-bit reading of one map (module doc): the straight line
/// between the two entries that bracket `x`, with `x` NOT clamped, so below 0
/// the first segment (0…1) continues along its slope and above 255 the last
/// (254…255) does. 16 bits read the 15-bit stage tables instead
/// ([`stage15`]), where Photoshop's render stays inside 0…255.
fn map_value_float(table: &[u8; 256], x: f64) -> f64 {
    // A NaN index lands on segment 0 and stays NaN through the arithmetic,
    // which the float render refuses with its own sentence.
    let i = x.floor().clamp(0.0, 254.0) as usize;
    let low = f64::from(table[i]);
    low + (x - i as f64) * (f64::from(table[i + 1]) - low)
}

/// Builds a 256-entry LUT through `points` (sorted, distinct `x`, len >= 2,
/// values in [0, 255]) by sampling the natural [`Spline`] at each code and
/// rounding. Inputs outside the point range take the nearest endpoint's
/// value.
fn spline_lut(points: &[(f64, f64)]) -> [u8; 256] {
    let spline = Spline::new(points);
    std::array::from_fn(|i| spline.value(i as f64).round() as u8)
}

/// The one natural cubic spline builder/evaluator (module doc). Byte LUTs
/// sample it at integer positions; typed processing evaluates its actual
/// input; the 32-bit float form continues it past its ends
/// ([`Spline::value_float`]). `m` holds the second derivatives `S''` at the
/// points, `m[0] = m[n − 1] = 0`.
#[derive(Clone)]
pub(crate) struct Spline {
    pub(crate) points: Vec<(f64, f64)>,
    pub(crate) m: Vec<f64>,
}
impl Spline {
    /// Solves the tridiagonal system for the interior second derivatives:
    /// `h[i−1]·M[i−1] + 2(h[i−1] + h[i])·M[i] + h[i]·M[i+1] = 6(d[i] − d[i−1])`
    /// with `h` the interval widths and `d` the secant slopes, by the Thomas
    /// algorithm (the matrix is strictly diagonally dominant, so it needs no
    /// pivoting). Parsing drops duplicate `x` values, so every interval is
    /// positive; near-equal ones give large but finite `M`.
    pub(crate) fn new(points: &[(f64, f64)]) -> Self {
        let n = points.len();
        let h: Vec<f64> = (0..n - 1).map(|k| points[k + 1].0 - points[k].0).collect();
        let d: Vec<f64> = (0..n - 1)
            .map(|k| (points[k + 1].1 - points[k].1) / h[k])
            .collect();
        let mut m = vec![0.0f64; n];
        if n > 2 {
            // Forward sweep over the interior rows 1..n−1: `c` the
            // eliminated super-diagonal, `r` the eliminated right side.
            let mut c = vec![0.0f64; n];
            let mut r = vec![0.0f64; n];
            for i in 1..n - 1 {
                let diagonal = 2.0 * (h[i - 1] + h[i]) - h[i - 1] * c[i - 1];
                c[i] = h[i] / diagonal;
                r[i] = (6.0 * (d[i] - d[i - 1]) - h[i - 1] * r[i - 1]) / diagonal;
            }
            for i in (1..n - 1).rev() {
                m[i] = r[i] - c[i] * m[i + 1];
            }
        }
        Self {
            points: points.to_vec(),
            m,
        }
    }

    /// `S(x)` on segment `k` (`x` in `[x_k, x_k+1]`), the standard
    /// second-derivative form.
    fn segment(&self, k: usize, x: f64) -> f64 {
        let (x0, y0) = self.points[k];
        let (x1, y1) = self.points[k + 1];
        let h = x1 - x0;
        let a = (x1 - x) / h;
        let b = (x - x0) / h;
        a * y0
            + b * y1
            + ((a * a * a - a) * self.m[k] + (b * b * b - b) * self.m[k + 1]) * h * h / 6.0
    }

    /// `S'` at the first point and at the last.
    fn end_slopes(&self) -> (f64, f64) {
        let n = self.points.len();
        let (x0, y0) = self.points[0];
        let (x1, y1) = self.points[1];
        let h0 = x1 - x0;
        let first = (y1 - y0) / h0 - h0 * (2.0 * self.m[0] + self.m[1]) / 6.0;
        let (xa, ya) = self.points[n - 2];
        let (xb, yb) = self.points[n - 1];
        let hn = xb - xa;
        let last = (yb - ya) / hn + hn * (self.m[n - 2] + 2.0 * self.m[n - 1]) / 6.0;
        (first, last)
    }

    /// The segment holding `x`, for `x` inside the point range.
    fn segment_at(&self, x: f64) -> usize {
        let n = self.points.len();
        self.points
            .partition_point(|p| p.0 <= x)
            .saturating_sub(1)
            .min(n - 2)
    }

    /// The 8/16-bit curve: flat beyond the ends, clamped to [0, 255].
    pub(crate) fn value(&self, x: f64) -> f64 {
        let points = &self.points;
        let n = points.len();
        if x <= points[0].0 {
            return points[0].1;
        }
        if x >= points[n - 1].0 {
            return points[n - 1].1;
        }
        self.segment(self.segment_at(x), x).clamp(0.0, 255.0)
    }

    /// The 32-bit curve (module doc): `S` inside the points with no clamp,
    /// the end slope above the last point, flat below the first unless the
    /// first sits at `x = 0`, where its slope continues. The ends were
    /// measured on take2's I1 (a first point at 0), I2 (a first point at 64)
    /// and A1 (a last point at 192).
    fn value_float(&self, x: f64) -> f64 {
        let n = self.points.len();
        let (x0, y0) = self.points[0];
        let (xn, yn) = self.points[n - 1];
        if x > xn {
            return yn + self.end_slopes().1 * (x - xn);
        }
        if x < x0 {
            return if x0 == 0.0 {
                y0 + self.end_slopes().0 * x
            } else {
                y0
            };
        }
        self.segment(self.segment_at(x), x)
    }
}

/// The identity lookup table (a missing curves channel).
fn identity_lut() -> [u8; 256] {
    std::array::from_fn(|i| i as u8)
}
