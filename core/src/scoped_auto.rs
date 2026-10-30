//! Levels Auto Tone / Auto Color measured over a BACKDROP: the
//! scoped-analysis request's `auto` field, answered beside the backdrop
//! histogram at 8 and 16 bits. The math is `ops_auto`'s — that module is the
//! master (`tone_endpoints`, `neutral_candidate`, `neutral_gamma_from_sums`); this
//! one only decides WHICH backdrop pixels vote and how much.
//!
//! # The weight
//!
//! Each canvas pixel votes with `selection × mask` (u16, an absent factor
//! reading 65535), multiplied left to right as
//! `mul(a, b) = (a * b + 32767) / 65535` in u32. The weight feeds the
//! derivation only; the histogram half of the answer keeps the caller's
//! coverage, so the plot does not move.
//!
//! * **selection** — the caller's coverage: a create dialog's captured
//!   selection, which becomes the new layer's mask.
//! * **mask** — edit mode only (no `insert`): the boundary layer's own
//!   effective mask on the canvas. Absent, disabled or full at every pixel is
//!   no factor.
//!
//! That scope was measured against Photoshop 26.10.0 (`scripts/
//! verify_levels_auto_photoshop.py`, the post-decision receipt): its
//! Levels-layer Auto counts the WHOLE visible backdrop below the layer even
//! when the layer is clipped, sits inside an isolated group, or a live
//! selection is active, and is limited only by the layer's own mask.
//! Neither a clip base nor an enclosing group narrows the measurement here.
//!
//! # Parity
//!
//! With no selection and no mask factor the weights are EMPTY and
//! the whole-scope units apply: 1 per pixel with the clip narrowed to f32 at
//! 8 bits (`ops_auto::auto_levels`), `u16::MAX` with the clip in f64 at 16
//! bits (`selected_auto_levels::AutoLevels::derive`), so the answers are
//! bit-identical to those commands over the same pixels.
//!
//! When no pixel has both weight and alpha the answer is `empty` with the
//! identity values. A 32-bit float document is refused at plan time with
//! `float_document::auto_refusal`.
//!
//! A Lab Color document (#547 phase 2f, `lab`): Auto Tone stretches the
//! Lightness plane alone (`ops_auto::contrast_endpoints` on index 0) and
//! a\* and b\* keep (0, 1, 1), `selected_auto_levels`' Lab rule; Auto Color
//! refuses (`lab_adjust::unavailable_sentence`).
//!
//! `scoped_auto.rs` answers a backdrop request's optional `auto` (Levels Auto Tone/Color,
//! `ops_auto`'s math, bit-identical to it and to `AutoLevels::derive` when nothing weights) beside
//! the unchanged histogram, over the WHOLE visible backdrop — the scope measured against Photoshop
//! 26.10.0, which a clip base, an isolated enclosing group or a live selection does not narrow
//! (`scripts/verify_levels_auto_photoshop.py`) — weighting each pixel only by the selection (a
//! create's future mask) × the edited adjustment's own mask; `insert` names a create slot that must
//! be the boundary, and a float document refuses at plan.

use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::ops_auto::{
    contrast_endpoints, neutral_candidate, neutral_gamma_from_sums, tone_endpoints,
};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::reserved_vec;
use crate::precision_structure_ops::FlatRecord;
use serde::Deserialize;
use serde_json::{json, Value};

/// Which Auto: per-channel endpoints (Tone), plus the neutral midtone snap
/// (Color).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoAnalysisMode {
    Tone,
    Color,
}

/// Where a NEW adjustment would land (create mode): the slot must be the
/// backdrop boundary.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoInsert {
    pub anchor: usize,
    pub position: crate::layer_insert::InsertPosition,
}

/// The request's `auto` object.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoAnalysis {
    pub mode: AutoAnalysisMode,
    #[serde(default = "default_clip")]
    pub clip: f64,
    #[serde(default)]
    pub insert: Option<AutoInsert>,
}

fn default_clip() -> f64 {
    0.001
}

/// The derivation's own scratch: three planes of `maximum + 1` u64 bins and
/// one u16 weight per canvas pixel.
pub(crate) fn scratch(maximum: u16, canvas: (u32, u32)) -> u64 {
    3 * (u64::from(maximum) + 1) * 8 + u64::from(canvas.0) * u64::from(canvas.1) * 2
}

fn mul(a: u16, b: u16) -> u16 {
    ((u32::from(a) * u32::from(b) + 32767) / 65535) as u16
}

/// The weight product over `count` pixels, left to right: selection ×
/// mask. `mask` is read per pixel and is passed only when it is a factor, so
/// the derivation holds at most ONE u16 plane. Empty when every factor is
/// absent (parity).
pub(crate) fn weights(
    count: usize,
    selection: &[u16],
    mask: Option<&dyn Fn(usize) -> u16>,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let Some(mask) = mask else {
        return Ok(selection.to_vec());
    };
    let mut out = reserved_vec(count)?;
    for i in 0..count {
        job.every(i)?;
        let factor = mask(i);
        out.push(if selection.is_empty() {
            factor
        } else {
            mul(selection[i], factor)
        });
    }
    Ok(out)
}

/// The Auto answer over `read`'s canvas-sized codes, weighted by `coverage`
/// (empty = the whole-scope units, module doc); `lab` is a Lab Color
/// document's Lightness rule (module doc).
pub(crate) fn derive(
    auto: &AutoAnalysis,
    dimensions: (u32, u32),
    maximum: u16,
    coverage: &[u16],
    read: &impl Fn(usize) -> [u16; 4],
    lab: bool,
    job: JobContext<'_>,
) -> Result<Value, String> {
    if lab && auto.mode == AutoAnalysisMode::Color {
        return Err(crate::lab_adjust::unavailable_sentence("Auto Color"));
    }
    let count = dimensions.0 as usize * dimensions.1 as usize;
    let unit = if maximum == u16::MAX {
        u64::from(u16::MAX)
    } else {
        1
    };
    let clip = if maximum == u16::MAX {
        auto.clip
    } else {
        f64::from(auto.clip as f32)
    };
    let weight = |i: usize| {
        if coverage.is_empty() {
            unit
        } else {
            u64::from(coverage[i])
        }
    };
    let size = usize::from(maximum) + 1;
    let mut bins = reserved_vec(3 * size)?;
    bins.resize(3 * size, 0_u64);
    let mut total = 0_u64;
    for i in 0..count {
        job.every(i)?;
        let n = weight(i);
        if n == 0 {
            continue;
        }
        let p = read(i);
        if p[3] == 0 {
            continue;
        }
        total += n;
        for c in 0..3 {
            bins[c * size + usize::from(p[c].min(maximum))] += n;
        }
    }
    let mut black = [0_f32; 3];
    let mut white = [1_f32; 3];
    let mut gamma = [1_f32; 3];
    if total > 0 {
        let cut = clip * total as f64;
        // One whole counted pixel's mass (`ops_auto::tone_endpoints`): the
        // whole-scope unit, or a full u16 weight.
        let whole = if coverage.is_empty() {
            unit as f64
        } else {
            f64::from(u16::MAX)
        };
        if lab {
            // Lightness alone; a* and b* keep (0, 1, 1) (module doc).
            if let Some((lo, hi)) = contrast_endpoints(&bins[..size], cut, whole) {
                black[0] = lo;
                white[0] = hi;
            }
        } else {
            let planes = [&bins[..size], &bins[size..2 * size], &bins[2 * size..]];
            (black, white) = tone_endpoints(planes, cut, whole, |add| {
                // The joint white's pixel pass replays the histogram's.
                for i in 0..count {
                    job.every(i)?;
                    let n = weight(i);
                    if n == 0 {
                        continue;
                    }
                    let p = read(i);
                    if p[3] == 0 {
                        continue;
                    }
                    add(
                        std::array::from_fn(|c| usize::from(p[c].min(maximum))),
                        n as f64,
                    );
                }
                Ok(())
            })?;
        }
        drop(bins);
        if auto.mode == AutoAnalysisMode::Color {
            // Row-major: the f64 sums are order-dependent.
            let mut sums = [0_f64; 3];
            let mut candidates = 0_u64;
            for i in 0..count {
                job.every(i)?;
                let n = weight(i);
                if n == 0 {
                    continue;
                }
                let p = read(i);
                if p[3] == 0 {
                    continue;
                }
                let rgb = std::array::from_fn::<_, 3, _>(|c| {
                    ((f32::from(p[c]) / f32::from(maximum) - black[c]) / (white[c] - black[c]))
                        .clamp(0., 1.)
                });
                if !neutral_candidate(rgb) {
                    continue;
                }
                candidates += n;
                for c in 0..3 {
                    sums[c] += f64::from(rgb[c]) * n as f64;
                }
            }
            gamma = neutral_gamma_from_sums(sums, candidates, total);
        }
    }
    job.check()?;
    let identity = black == [0.; 3] && white == [1.; 3] && gamma == [1.; 3];
    Ok(json!({
        "mode": match auto.mode {
            AutoAnalysisMode::Tone => "tone",
            AutoAnalysisMode::Color => "color",
        },
        "clip": auto.clip,
        "empty": total == 0,
        "black": black,
        "white": white,
        "gamma": gamma,
        "identity": identity,
    }))
}

const AUTO_SLOT: &str = "auto insert must land at the backdrop boundary";
const AUTO_ADJUSTMENT: &str =
    "auto without insert needs an adjustment layer at the backdrop boundary";

impl PrecisionDocument {
    /// Whether an `auto` is an EDIT (no `insert`: record `below` must be an
    /// adjustment, whose own mask then weighs) or a create (`insert`: the
    /// slot it names must be `below`).
    pub(crate) fn auto_edits(
        &self,
        flat: &[FlatRecord],
        auto: &AutoAnalysis,
        below: usize,
    ) -> Result<bool, String> {
        match &auto.insert {
            Some(insert) => {
                if self.insert_slot(insert.anchor, insert.position)?.at != below {
                    return Err(AUTO_SLOT.into());
                }
                Ok(false)
            }
            None => {
                flat.get(below)
                    .filter(|r| matches!(r.layer.content, Content::Adjustment(_)))
                    .ok_or(AUTO_ADJUSTMENT)?;
                Ok(true)
            }
        }
    }

    /// The `auto` weights (module doc) at 16 bits: the edited layer's
    /// effective mask, `round(at · 65535)`, folded onto the caller's coverage.
    pub(crate) fn auto_weights(
        &self,
        flat: &[FlatRecord],
        auto: &AutoAnalysis,
        below: usize,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let edit = self.auto_edits(flat, auto, below)?;
        let width = self.width as usize;
        let count = width * self.height as usize;
        let prepared = edit
            .then(|| flat[below].layer.mask())
            .flatten()
            .filter(|m| m.enabled())
            .map(|m| m.prepare(job))
            .transpose()?;
        let Some(prepared) = prepared else {
            return weights(count, coverage, None, job);
        };
        // Evaluate the mask once, cancellably; an all-opaque mask is no factor.
        let mut factors = reserved_vec(count)?;
        for i in 0..count {
            job.every(i)?;
            let at = prepared.at((i % width) as i64, (i / width) as i64);
            factors.push((at * 65535.).round() as u16);
        }
        if factors.iter().all(|&f| f == u16::MAX) {
            drop(factors);
            return weights(count, coverage, None, job);
        }
        // Fold the selection in place (selection × mask, as `weights`), so
        // the derivation holds ONE u16 plane, as the plan charges.
        if !coverage.is_empty() {
            for (i, factor) in factors.iter_mut().enumerate() {
                job.every(i)?;
                *factor = mul(coverage[i], *factor);
            }
        }
        Ok(factors)
    }
}

impl DrDocument {
    /// The byte twin of `PrecisionDocument::auto_edits`.
    pub(crate) fn auto_edits(&self, auto: &AutoAnalysis, below: usize) -> Result<bool, String> {
        match &auto.insert {
            Some(insert) => {
                if self.insert_slot(insert.anchor, insert.position)?.at != below {
                    return Err(AUTO_SLOT.into());
                }
                Ok(false)
            }
            None => {
                self.layers
                    .get(below)
                    .filter(|l| crate::retouch_sampling::is_adjustment(l))
                    .ok_or(AUTO_ADJUSTMENT)?;
                Ok(true)
            }
        }
    }

    /// The byte twin of `PrecisionDocument::auto_weights`: the edited
    /// layer's effective mask `m * 257`.
    pub(crate) fn auto_weights(
        &self,
        auto: &AutoAnalysis,
        below: usize,
        coverage: &[u16],
        job: JobContext<'_>,
    ) -> Result<Vec<u16>, String> {
        let edit = self.auto_edits(auto, below)?;
        let count = self.width as usize * self.height as usize;
        let crop = match self.layers.get(below).filter(|l| edit && l.mask_enabled) {
            Some(layer) => {
                layer.effective_mask_rect_with_job((0, 0), self.width, self.height, job)?
            }
            None => None,
        };
        let at = |i: usize| {
            crop.as_ref()
                .map_or(u16::MAX, |c| u16::from(c.as_raw()[i]) * 257)
        };
        let masked = crop
            .as_ref()
            .is_some_and(|c| c.as_raw().iter().any(|&v| v != 255));
        weights(
            count,
            coverage,
            masked.then_some(&at as &dyn Fn(usize) -> u16),
            job,
        )
    }
}
