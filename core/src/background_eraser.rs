//! The Background Eraser's coverage (`designs/background-eraser.md`): under each dab of a
//! stroke, the pixels whose colour is like a sampled colour take the dab's tip coverage,
//! the rest take none. A kernel: it names no document type. The erase itself is the
//! existing one (the byte paint walk's Clear, the typed `erase_native`); this module only
//! decides how much of each pixel is erased.
//!
//! **The rule.** Each dab with a nonzero diameter and opacity has a HOT SPOT, the pixel
//! holding its centre, and a REFERENCE colour: its own hot-spot pixel (`continuous`), the
//! hot-spot pixel of the stroke's first dab whose hot spot is on the canvas and not fully
//! transparent (`once`), or the request's `swatch` (`swatch`). A dab with no reference
//! (its hot spot off the canvas or fully transparent, or a `once` stroke before any such
//! dab) erases nothing. A pixel is LIKE the reference when the Magic Wand would admit it
//! from a seed of that colour: R, G and B within the tolerance and the halved cyclic
//! differences within half of it, on bytes at every depth
//! (`selection_similar::ReferenceRange` over its `keys`, the wand's key form and
//! **Resolution**), or a Lab document's key
//! (`lab_select::LabWandRange`). Both colours are compared with their alpha read as full,
//! so a half-erased pixel stays like its unerased neighbours and a second pass keeps
//! erasing it; a fully transparent pixel (alpha code 0) is never admitted. With `protect`
//! a pixel like the protected colour (the same predicate and tolerance) is never admitted.
//!
//! **Limits.** `discontiguous` admits every like pixel under the tip. `contiguous` admits
//! the like pixels four-connected to the hot spot through like pixels the tip covers,
//! inside the dab's box (`selection_region::flood`, the wand's walk); a dab whose hot spot
//! is not itself admitted (unlike the reference, protected, or uncovered by its own tip)
//! erases nothing. `flood` marks its seed whatever its predicate says, so that guard is
//! this module's: without it a protected hot spot would be erased under every dab.
//!
//! **Coverage.** An admitted pixel takes `c = tip × dab opacity`, the tip coverage being
//! `BrushRequest::dab_coverage` (exactly what `rasterize` lays for that dab); dabs
//! accumulate `old += c·(1 − old)` in order and the field is quantized once
//! (`BrushRequest::quantize`). With every pixel admitted the field is `rasterize`'s, sample
//! for sample. Every dab samples the layer as it was before the stroke, so the result does
//! not depend on how the stroke was split into ticks.
//!
//! **Work.** One stroke plans at most [`MAX_SMUDGE_WORK`] units: the footprint once (the
//! field) and per dab its tip charge (`BrushRequest::dab_work`) plus its clipped box (the
//! match and the walk). It is Smudge's bound for Smudge's reason: the 8-bit canvas reruns
//! the whole stroke from the gesture's base on every tick, synchronously.
use crate::brush_coverage::{bounds, footprint, BrushRequest, MAX_SMUDGE_WORK};
use crate::job_control::JobContext;
use crate::lab_blend::LabGrid;
use crate::lab_select::LabWandRange;
use crate::precision_image::{reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_paint::PaintColorSpace;
use crate::selection_region::flood;
use crate::selection_similar::{keys, ReferenceRange};
use serde::Deserialize;

/// [`MAX_SMUDGE_WORK`]'s refusal for this tool.
const WORK_REFUSAL: &str =
    "background eraser exceeds 256 million work units; use a shorter stroke or a smaller brush";

/// Which pixels under the tip a dab may reach (module doc, **Limits**).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Limits {
    Contiguous,
    Discontiguous,
}

/// Where each dab's reference colour comes from (module doc, **The rule**).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sampling {
    Continuous,
    Once,
    Swatch,
}

/// A reference colour as the request carries it: straight RGBA in 0…1 with its
/// `color_space` (a CMYK document refuses it), or a CMYK document's straight
/// `[c, m, y, k, alpha]` ink in 0…1. The adapters turn it into the codes their
/// document compares.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reference {
    #[serde(default)]
    pub(crate) color: Option<[f32; 4]>,
    #[serde(default)]
    pub(crate) color_space: Option<PaintColorSpace>,
    #[serde(default)]
    pub(crate) ink: Option<[f32; 5]>,
}

/// The tool's request: `{"version":1,"tolerance":0..65535,"limits":L,"sampling":S}` with
/// the optional `swatch` (required with `sampling: "swatch"`, refused otherwise) and
/// `protect` colours. The tolerance is in u16 codes (the host's 8-bit Tolerance × 257).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackgroundEraserRequest {
    version: u32,
    pub(crate) tolerance: u16,
    pub(crate) limits: Limits,
    pub(crate) sampling: Sampling,
    #[serde(default)]
    pub(crate) swatch: Option<Reference>,
    #[serde(default)]
    pub(crate) protect: Option<Reference>,
}

impl BackgroundEraserRequest {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let request: Self = serde_json::from_str(text)
            .map_err(|e| format!("invalid background eraser request: {e}"))?;
        if request.version != 1 {
            return Err("background eraser request requires version 1".into());
        }
        match (request.sampling, request.swatch.is_some()) {
            (Sampling::Swatch, false) => {
                Err("background eraser swatch sampling requires a swatch colour".into())
            }
            (Sampling::Continuous | Sampling::Once, true) => {
                Err("background eraser takes a swatch colour only with swatch sampling".into())
            }
            _ => Ok(request),
        }
    }
}

/// The like-colour predicate a document compares on (module doc).
#[derive(Clone, Copy, Debug)]
pub(crate) enum LikeRule {
    /// The wand's key form on the keys of straight u16 codes (R, G and B as their
    /// nearest bytes, `selection_similar::keys`).
    Rgb,
    /// A Lab document's wand key on its stored codes.
    Lab(LabGrid),
}

/// One reference colour's range under `rule`.
enum Matcher {
    Rgb(ReferenceRange),
    Lab(LabWandRange, LabGrid),
}

/// `p` with its alpha read as full (module doc).
fn opaque(p: [u16; 4]) -> [u16; 4] {
    [p[0], p[1], p[2], u16::MAX]
}

impl Matcher {
    fn new(rule: LikeRule, reference: [u16; 4]) -> Self {
        match rule {
            // The seed is keyed as the wand's is (`selection_region::grow`); `admits`
            // keys the pixel itself.
            LikeRule::Rgb => Self::Rgb(ReferenceRange::from_seed(keys(opaque(reference)))),
            LikeRule::Lab(grid) => {
                Self::Lab(LabWandRange::from_seed(opaque(reference), grid), grid)
            }
        }
    }
    fn admits(&self, p: [u16; 4], tolerance: u16) -> bool {
        match self {
            Self::Rgb(range) => range.admits(opaque(p), tolerance),
            Self::Lab(range, grid) => range.admits(opaque(p), tolerance, *grid),
        }
    }
}

/// The swatch and protect colours as compare codes (alpha ignored).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReferenceCodes {
    pub(crate) swatch: Option<[u16; 4]>,
    pub(crate) protect: Option<[u16; 4]>,
}

/// What one stroke will cost: the work units (module doc, **Work**), the
/// footprint `[x0, y0, x1, y1)` (`None` when no dab reaches the canvas), and
/// the kernel's own scratch: the field and the placed samples over the
/// footprint, and the largest dab box's tip, walk plane and queue.
pub(crate) struct Plan {
    pub(crate) work_units: u64,
    pub(crate) footprint: Option<[usize; 4]>,
    pub(crate) scratch_bytes: u64,
}

/// The plan of a stroke over a `dimensions` canvas: the brush's own checks
/// (`BrushRequest::plan`: the dab count and geometry), then this tool's work
/// bound, refused by name past [`MAX_SMUDGE_WORK`].
pub(crate) fn plan(
    brush: &BrushRequest,
    dimensions: (u32, u32),
    dabs: &[f64],
) -> Result<Plan, String> {
    brush.plan(dimensions, dabs)?;
    let area = |b: [usize; 4]| (b[2] - b[0]) as u64 * (b[3] - b[1]) as u64;
    let footprint = footprint(dimensions, dabs);
    let mut work = footprint.map_or(0, area);
    let mut largest = 0;
    for d in dabs.chunks_exact(4) {
        if d[2] == 0. || d[3] == 0. {
            continue;
        }
        let pixels = area(bounds(d, dimensions));
        largest = largest.max(pixels);
        work = work.saturating_add(brush.dab_work(dimensions, d) + pixels);
        if work > MAX_SMUDGE_WORK {
            return Err(WORK_REFUSAL.into());
        }
    }
    Ok(Plan {
        work_units: work,
        footprint,
        scratch_bytes: 4 * BOOKKEEPING_BYTES
            + footprint.map_or(0, area) * 16
            + largest * (8 + 1 + 4),
    })
}

/// Footprint-sized straight codes of a layer of `layer` dimensions placed at
/// canvas `offset`, row-major, `[0; 4]` (transparent) where the layer does not
/// reach: `run(ly, lx0, lx1, out)` appends the `lx1 − lx0` codes of layer row
/// `ly`'s columns `lx0..lx1`.
pub(crate) fn placed(
    footprint: [usize; 4],
    layer: (u32, u32),
    offset: (i32, i32),
    mut run: impl FnMut(u32, u32, u32, &mut Vec<[u16; 4]>) -> Result<(), String>,
    job: JobContext<'_>,
) -> Result<Vec<[u16; 4]>, String> {
    let [fx0, fy0, fx1, fy1] = footprint.map(|v| v as i64);
    let (lw, lh) = (i64::from(layer.0), i64::from(layer.1));
    let (ox, oy) = (i64::from(offset.0), i64::from(offset.1));
    let row = (fx1 - fx0) as usize;
    let mut out = reserved_vec(row * (fy1 - fy0) as usize)?;
    for y in fy0..fy1 {
        job.check()?;
        let start = out.len();
        let ly = y - oy;
        let lx0 = (fx0 - ox).clamp(0, lw);
        let lx1 = (fx1 - ox).clamp(0, lw);
        if (0..lh).contains(&ly) && lx0 < lx1 {
            out.resize(start + (lx0 + ox - fx0) as usize, [0; 4]);
            run(ly as u32, lx0 as u32, lx1 as u32, &mut out)?;
        }
        out.resize(start + row, [0; 4]);
    }
    Ok(out)
}

/// The stroke's unquantized coverage field over `footprint` (from [`plan`]),
/// row-major: the module doc's rule over `samples`, the layer's straight codes
/// placed over the same footprint ([`placed`]), compared by `rule` with the
/// request's tolerance and its `references` as compare codes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn field(
    request: &BackgroundEraserRequest,
    brush: &BrushRequest,
    dimensions: (u32, u32),
    dabs: &[f64],
    footprint: [usize; 4],
    samples: &[[u16; 4]],
    rule: LikeRule,
    references: ReferenceCodes,
    job: JobContext<'_>,
) -> Result<Vec<f64>, String> {
    let [fx0, fy0, fx1, fy1] = footprint;
    let fw = fx1 - fx0;
    if samples.len() != fw * (fy1 - fy0) {
        return Err("background eraser samples differ from the footprint".into());
    }
    let mut field = reserved_vec(samples.len())?;
    field.resize(samples.len(), 0_f64);
    let tolerance = request.tolerance;
    let protect = references.protect.map(|p| Matcher::new(rule, p));
    let rotation = brush.rotation();
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let mut once: Option<[u16; 4]> = None;
    let count = (dabs.len() / 4).max(1);
    for (di, d) in dabs.chunks_exact(4).enumerate() {
        job.check()?;
        if d[2] == 0. || d[3] == 0. {
            continue;
        }
        let [x0, y0, x1, y1] = bounds(d, dimensions);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let local = |x: usize, y: usize| (y - fy0) * fw + (x - fx0);
        let (hx, hy) = (d[0].floor(), d[1].floor());
        let hot = (hx >= 0. && hy >= 0. && (hx as usize) < w && (hy as usize) < h)
            .then_some((hx as usize, hy as usize))
            .filter(|&(x, y)| (x0..x1).contains(&x) && (y0..y1).contains(&y));
        let hot_sample = hot.map(|(x, y)| samples[local(x, y)]).filter(|p| p[3] != 0);
        let reference = match request.sampling {
            Sampling::Continuous => hot_sample,
            Sampling::Once => {
                once = once.or(hot_sample);
                once
            }
            Sampling::Swatch => references.swatch,
        };
        let Some(reference) = reference else {
            continue;
        };
        let like = Matcher::new(rule, reference);
        let admits = |p: [u16; 4]| {
            p[3] != 0
                && like.admits(p, tolerance)
                && !protect.as_ref().is_some_and(|m| m.admits(p, tolerance))
        };
        // The dab's tip over its clipped box, before its opacity.
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut tip = reserved_vec(bw * bh)?;
        for y in y0..y1 {
            job.check()?;
            for x in x0..x1 {
                tip.push(brush.dab_coverage(d, rotation, x, y));
            }
        }
        let inside = |k: usize| tip[k] > 0. && admits(samples[local(x0 + k % bw, y0 + k / bw)]);
        let reached: Option<Vec<u8>> = match request.limits {
            Limits::Discontiguous => None,
            Limits::Contiguous => {
                let Some((x, y)) = hot else { continue };
                let seed = (y - y0) * bw + (x - x0);
                if !inside(seed) {
                    continue;
                }
                Some(flood(
                    (bw as u32, bh as u32),
                    [seed],
                    inside,
                    true,
                    JobContext::NONE,
                )?)
            }
        };
        for k in 0..bw * bh {
            job.every(k)?;
            let admitted = match &reached {
                Some(mask) => mask[k] != 0,
                None => inside(k),
            };
            if admitted {
                let c = tip[k] * d[3];
                let old = &mut field[local(x0 + k % bw, y0 + k / bw)];
                *old += c * (1. - *old);
            }
        }
        job.report((di + 1) as f64 / count as f64 * 0.9, 3)?;
    }
    Ok(field)
}
