//! KERNEL: Image ▸ Adjustments ▸ Equalize (#547) — the weighted histogram,
//! its look-up table and the request's scope. Free functions over plain
//! numbers; the document adapters are `equalize_apply` (byte image, RGB16,
//! f32 and CMYK layers).
//!
//! The rule is EQ-SUM-MIN, the declared H0 of the Photoshop take
//! (`scripts/photoshop-probe/cmyk_retouch_take.py`):
//!
//! * ONE table from the SUM of the colour planes' histograms — R, G and B on
//!   an RGB layer, the four `L = 1 − ink` plates on a CMYK one — so every
//!   plane is remapped by the same curve and a neutral stays neutral;
//! * `lut[v] = round(max · (cdf(v) − cdf(v_min)) / (N − cdf(v_min)))`, where
//!   `cdf(v)` is the weight at or below level `v`, `v_min` the darkest
//!   occupied level, `N` the total weight and `max` the top level. The
//!   darkest occupied level maps to 0 and the brightest to `max`; a level
//!   below `v_min` (never occupied) maps to 0. The table is monotone, since
//!   `cdf` is.
//!
//! Each sample's weight is the caller's (selection coverage × alpha, both in
//! 0…1); a sample of weight 0 adds nothing. Fewer than two occupied levels
//! have no spread to equalize: [`Histogram::lut`] answers `None`, and the
//! adapters answer "nothing changes".
//!
//! Levels are 256 for 8-bit data and 65536 for 16-bit codes and the float
//! display codes; [`Histogram::new`] refuses any other count. The weights
//! accumulate in f64 (a 2³¹-pixel layer of three planes sums exactly to
//! well under 2⁵³ at unit weight).

use serde_json::{Map, Value};

/// A weighted histogram over `bins.len()` levels.
pub(crate) struct Histogram {
    bins: Vec<f64>,
}

impl Histogram {
    /// An empty histogram of `levels` levels: 256 or 65536.
    pub(crate) fn new(levels: usize) -> Result<Self, String> {
        if levels != 256 && levels != 65536 {
            return Err("an equalize histogram has 256 or 65536 levels".into());
        }
        Ok(Self {
            bins: vec![0.0; levels],
        })
    }

    /// Adds `weight` at `level`; a level past the top counts at the top, and
    /// a weight that is not positive and finite adds nothing.
    pub(crate) fn add(&mut self, level: usize, weight: f64) {
        if !(weight.is_finite() && weight > 0.0) {
            return;
        }
        let top = self.bins.len() - 1;
        self.bins[level.min(top)] += weight;
    }

    /// The first level whose cumulative weight reaches `fraction` of the
    /// total (HDR Toning's Local Adaptation reads its log range's low end
    /// here, `hdr_local_adaptation`); `None` when the histogram is empty. An
    /// empty level never answers, so `fraction` 0 is the darkest occupied one.
    pub(crate) fn quantile(&self, fraction: f64) -> Option<usize> {
        let total: f64 = self.bins.iter().sum();
        if total <= 0.0 {
            return None;
        }
        let target = fraction * total;
        let mut cdf = 0.0;
        for (level, &weight) in self.bins.iter().enumerate() {
            cdf += weight;
            if weight > 0.0 && cdf >= target {
                return Some(level);
            }
        }
        self.bins.iter().rposition(|&w| w > 0.0)
    }

    /// The module doc's table, `None` with fewer than two occupied levels.
    pub(crate) fn lut(&self) -> Option<Vec<u16>> {
        let first = self.bins.iter().position(|&w| w > 0.0)?;
        let last = self.bins.iter().rposition(|&w| w > 0.0)?;
        if first == last {
            return None;
        }
        let total: f64 = self.bins.iter().sum();
        let floor = self.bins[first];
        let span = total - floor;
        if span <= 0.0 {
            return None;
        }
        let max = (self.bins.len() - 1) as f64;
        let mut cdf = 0.0;
        Some(
            self.bins
                .iter()
                .map(|&w| {
                    cdf += w;
                    let t = ((cdf - floor) / span).clamp(0.0, 1.0);
                    (max * t).round() as u16
                })
                .collect(),
        )
    }
}

/// Which pixels the table is applied to when a selection exists: only the
/// selected area, or the whole target from the selected area's histogram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EqualizeScope {
    Selected,
    Entire,
}

/// The request's `params`: an optional `scope`, `"selected"` (the default)
/// or `"entire"`; any other key refuses.
pub(crate) fn parse_scope(params: &Map<String, Value>) -> Result<EqualizeScope, String> {
    if let Some(key) = params.keys().find(|k| k.as_str() != "scope") {
        return Err(format!("{key} is not an argument of equalize"));
    }
    match params.get("scope") {
        None => Ok(EqualizeScope::Selected),
        Some(Value::String(s)) if s == "selected" => Ok(EqualizeScope::Selected),
        Some(Value::String(s)) if s == "entire" => Ok(EqualizeScope::Entire),
        Some(_) => Err("equalize scope must be selected or entire".into()),
    }
}
