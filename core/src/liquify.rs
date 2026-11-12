//! Liquify: brush strokes that push, pinch, swell, twirl, restore,
//! freeze and thaw a displacement field, and the bicubic render of a picture
//! through that field. Free functions and one value type over plain samples;
//! `selected_filter` (16 bits), `ops_spatial::filter_op` (8 bits) and
//! `ffi_liquify` (the workspace's live field) are the adapters.
//!
//! THE FIELD. A plane of `W × H` pixels carries a grid of nodes `cell` plane
//! pixels apart, `cell = ⌈max(W, H) / FIELD_MAX_SIDE⌉`, so a plane up to 2048
//! pixels on its longest side has one node per pixel and a larger one keeps
//! at most 2048 nodes a side. Node `(i, j)` sits at the plane point
//! `((i + ½)·cell, (j + ½)·cell)` and holds a displacement `D` in plane pixels
//! and a freeze `F` in 0…1. `D` and `F` at any point are the bilinear
//! interpolation of the nodes, node indexes clamped at the borders.
//!
//! THE RENDER. The output pixel whose centre is the plane point `p` is the
//! source sampled at `p + D(p)`: `D` says where a pixel's content comes FROM,
//! so pushing content east by 5 stores `D = (−5, 0)`. The sample is
//! Catmull-Rom over premultiplied samples through `sampling::map_into` (the
//! one tap walk); the point is clamped to the source's pixel centres, so a
//! sample from beyond the plane repeats the edge pixel, and taps outside the
//! picture are dropped and the rest renormalized. A pixel whose `D` is exactly
//! zero is its source sample as read, hidden colour under alpha 0 included.
//! On a linear-light 8- or 16-bit document the caller hands the transfer
//! (`sampling_linear::LinearUnit`): each tap's RGB is decoded before it is
//! premultiplied and the clamped result encoded once (11E D7); a pixel whose
//! `D` is zero is still its sample as read.
//!
//! A STROKE is a list of points and becomes dabs: with `r = max(size / 2,
//! 1.5·cell)` (a brush smaller than three nodes across would miss the grid)
//! and `step = max(cell, r / 4)`, point 0 is one dab with no motion (unless
//! the stroke is `continued`), and a segment of length `L` is `k = ⌈L / step⌉`
//! evenly spaced dabs ending on its far point, each with motion `m = segment /
//! k` and weight `ω = L / (k·step)`; a zero-length segment is one dab of
//! weight 1 with no motion (a dwell). Segments never share a remainder, so a
//! stroke applied whole equals its point 0 followed by its segments as
//! `continued` two-point strokes: the workspace applies a drag one segment at
//! a time and the committed edit replays the same dabs. The step is a quarter
//! of the radius because every point of the path is then inside eight
//! overlapping dabs, so a drag reads as one continuous push and not as a row
//! of discs, while the dab count, and with it the work, stays the path length
//! over an eighth of the brush size; it is never under a cell, since a finer
//! step moves the field by less than the node spacing that can hold it.
//!
//! A DAB at centre `c` touches every node `q` with `t = |q − c| / r < 1`. Its
//! falloff is `f = 1` for `t ≤ d` and the smoothstep `s²(3 − 2s)`, `s = (1 −
//! t) / (1 − d)`, beyond, with `d = 0.9·density`; `a = strength·f·(1 − F(q))`.
//! The 0.9 keeps a falloff over the outer tenth of the radius at density 1:
//! a brush at full strength up to its rim would leave a step in the field
//! there, and a step in `D` renders as a tear in the picture. With `u = q − c`
//! and every `D` read from the field as it stood before the dab, a moving
//! tool makes a small step `e` and COMPOSES it with the field,
//! `D′(q) = D(q + e) + e` (sample where the step points, then follow the older
//! displacement from there), which is what keeps repeated dabs from tearing:
//! Forward Warp `e = −a·m`; Pucker `e = a·ω·RATE·u`; Bloat `e = −a·ω·RATE·u`;
//! Twirl `e = R(∓a·ω·TWIRL_RADIANS)·u − u` (clockwise on the y-down plane
//! samples from the counterclockwise side). Reconstruct scales the
//! displacement toward zero, `D′ = D·(1 − min(1, a·ω))`; Freeze and Thaw move
//! `F` toward 1 and 0 by `min(1, strength·f·ω)`.
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling::{map_into, Kernel, Unorm};
use crate::sampling_linear::LinearUnit;
use image::RgbaImage;
use serde_json::{Map, Value};
use std::cell::Cell;

/// The most nodes a field keeps along its longer side. 2048 is the workspace
/// proxy's longest side too, so a proxy pixel never falls between two nodes of
/// a coarser grid than it can show, and a full field is 4.2 million nodes
/// (80 MiB at [`BYTES_PER_NODE`]) however large the plane.
pub(crate) const FIELD_MAX_SIDE: u32 = 2048;
pub(crate) const MAX_STROKES: usize = 2048;
pub(crate) const MAX_STROKE_POINTS: usize = 8192;
pub(crate) const MAX_POINTS: usize = 131_072;
/// The byte cap of a liquify request's JSON text (the `params_json` of
/// `dr_image_filter_op` for `liquify`, and `dr_liquify_field_apply`'s
/// `strokes_json`). A point pair serializes in at most 50 bytes (two
/// 17-significant-digit doubles with sign and exponent, brackets, commas), so
/// [`MAX_POINTS`] pairs are 6,553,600 bytes, and [`MAX_STROKES`] stroke
/// headers of at most 160 bytes are 327,680; 6,881,280 < 8,388,608. Every
/// list the three count caps admit therefore fits, whatever the layer origin
/// subtracted from its points.
pub(crate) const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
/// The cap on a field's summed dab boxes, in field nodes: the one bound on
/// what a stroke list can cost, since neither the stroke count nor the point
/// count bounds the dabs of a long segment. A full-field dab is 4.2 million
/// nodes, so this admits 238 of them, or 25,000 dabs of a 200-node brush.
///
/// There is no work-ceiling comparison beside it: the dab sum is at most
/// `MAX_WORK` and the render is `16·n` with n ≤ `doc::MAX_PIXELS` (1e8),
/// 2.6e9 in all, below `sampling_gaussian::MAX_DESTRUCTIVE_WORK` (24e9), the
/// smallest ceiling any caller holds.
pub(crate) const MAX_WORK: u64 = 1_000_000_000;
/// Pucker's and Bloat's step per unit-weight dab, as a fraction of the node's
/// distance from the dab centre: ten dabs at full strength move a point about
/// two thirds of the way in, so a held brush acts in under a second at the
/// workspace's 15 dwells a second without collapsing in one tick.
pub(crate) const RATE: f64 = 0.1;
/// Twirl's angle per unit-weight dab, in radians (8.6°): a quarter turn takes
/// about ten full-strength dabs, the same pace as [`RATE`].
pub(crate) const TWIRL_RADIANS: f64 = 0.15;
/// [`plan`]'s per-pixel term: the n×16 plane [`plane`] returns is the only
/// buffer that grows with the picture (the source is the caller's own).
///
/// Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
/// instance per case, `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one `liquify` call,
/// 3 runs + a baseline twin; a one-layer document of two-colour Clouds):
/// - 6000×4000, one `forward_warp` of size 600 from (1000, 2000) to
///   (5000, 2000). This plan is 16n + 20 × 2,668,000 nodes + 2 MiB =
///   439,457,152 and binds at neither depth.
///   - 16-bit: E 576,504,216 / 576,618,904 / 576,651,672 against
///     `selected_edit_plan` 822,291,456 (1.426×), which its 24n combine stage
///     sets.
///   - 8-bit: E 789,037,608 / 782,483,960 / 762,298,920: the peak is the
///     commit's, which the host charges. `ByteFilterAdmission.commitStageBytes`
///     is 8n + the margined projection, 8n + 24n + ⌈7 × 24n / 16⌉ =
///     1,020,000,000: the core's `projection_scratch_bytes` is 20n for a
///     one-layer document (each measured instance's `resource_usage` reported
///     480,000,000), the display payload is 4n, and `ProjectionMargin` adds
///     7/16. That is 1.293×, before what the edit leaves retained (14,159,514
///     to 110,159,554 across runs). `filter_op_scratch_bytes` is 605,242,880,
///     its 20n combine stage.
/// - 2048×2048, one `forward_warp` of size 5000 from (400, 1024) to
///   (1600, 1024): one node a pixel and a brush over the whole grid, the
///   plane of the most pixels on which this plan, 36n + 2 MiB = 153,092,096,
///   is the term that admits at both depths.
///   - 16-bit: E 117,686,344 / 117,735,496 / 117,686,344 against
///     `selected_edit_plan` 201,326,592 (1.710×). E is 28n, the 12n of nodes
///     beside the 16n plane: the dab's scratch is freed before the plane is
///     made, so the 8 bytes a node this plan adds for it are margin.
///   - 8-bit: E 150,913,072 / 150,880,304 / 141,721,648 against
///     `filter_op_scratch_bytes` 196,608,000 (1.303×; `commitStageBytes` is
///     178,257,920, below it), before the 33,555,650 the edit leaves retained.
/// - Twins: E 0, and 16,384 for the 2048×2048 16-bit one.
const PLANE_BYTES_PER_PIXEL: u64 = 16;
/// [`plan`]'s per-node term: the displacement (two f32, 8 bytes), the freeze
/// (one f32, 4) and the dab's scratch copy of the box it rewrites (two f32 a
/// node, at most the whole grid, 8).
///
/// Record: [`PLANE_BYTES_PER_PIXEL`]'s. The 6000×4000 field is 2000×1334
/// nodes, 53,360,000 bytes of this term, in a plan a later stage sets at both
/// depths; the 2048×2048 field is 4,194,304 nodes, 83,886,080 bytes of the
/// 153,092,096 that admit there.
const BYTES_PER_NODE: u64 = 20;

/// Rows of one [`LiquifyField::render_rgba8`] band: the float samples of a
/// band are quantized into the output before the next is rendered, so the
/// byte render holds 16 bytes a pixel for one band, never for the picture.
const RENDER_BAND_ROWS: u32 = 64;

const STROKES_REFUSAL: &str = "liquify needs strokes: an array of 1 to 2048 strokes";
const TOOL_REFUSAL: &str = "liquify tool must be one of forward_warp, pucker, bloat, \
    twirl_clockwise, twirl_counterclockwise, reconstruct, freeze, thaw";
const POINTS_REFUSAL: &str = "liquify points must be 1 to 8192 finite [x,y] pairs within 1000000";
const WORK_REFUSAL: &str = "liquify strokes exceed the limit of 1 billion field updates; \
    use fewer or shorter strokes or a smaller brush";
const RECT_REFUSAL: &str = "liquify render rect must be a non-empty rectangle inside the image";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiquifyTool {
    ForwardWarp,
    Pucker,
    Bloat,
    TwirlClockwise,
    TwirlCounterclockwise,
    Reconstruct,
    Freeze,
    Thaw,
}

/// One stroke in PLANE pixels. `size` is the brush diameter (1…5000),
/// `strength` and `density` are 0…1, and a `continued` stroke has no dab at
/// its point 0 (it carries on from a stroke that ended there).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LiquifyStroke {
    pub tool: LiquifyTool,
    pub size: f64,
    pub strength: f64,
    pub density: f64,
    pub continued: bool,
    pub points: Vec<[f64; 2]>,
}

/// The kernel JSON `{"strokes":[{"tool","size","strength","density","points",
/// "continued"}]}`, strictly: 1…2048 strokes; `tool` one of the eight names;
/// `size` 1…5000; `strength` and `density` 0…1 (default 0.5); `points` 1…8192
/// `[x, y]` pairs within ±1,000,000; `continued` a boolean (default false);
/// at most 131,072 points in all. `Err` names what is wrong. A JSON boolean is
/// not a number.
pub(crate) fn parse_strokes(params: &Map<String, Value>) -> Result<Vec<LiquifyStroke>, String> {
    if let Some(key) = params.keys().find(|k| k.as_str() != "strokes") {
        return Err(format!("unexpected liquify parameter {key}"));
    }
    let list = params
        .get("strokes")
        .and_then(Value::as_array)
        .filter(|list| (1..=MAX_STROKES).contains(&list.len()))
        .ok_or(STROKES_REFUSAL)?;
    let mut strokes = Vec::with_capacity(list.len());
    let mut total = 0_usize;
    for value in list {
        let stroke = value.as_object().ok_or(STROKES_REFUSAL)?;
        const KEYS: [&str; 6] = ["tool", "size", "strength", "density", "points", "continued"];
        if let Some(key) = stroke.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(format!("unexpected liquify stroke parameter {key}"));
        }
        let tool = match stroke.get("tool").and_then(Value::as_str) {
            Some("forward_warp") => LiquifyTool::ForwardWarp,
            Some("pucker") => LiquifyTool::Pucker,
            Some("bloat") => LiquifyTool::Bloat,
            Some("twirl_clockwise") => LiquifyTool::TwirlClockwise,
            Some("twirl_counterclockwise") => LiquifyTool::TwirlCounterclockwise,
            Some("reconstruct") => LiquifyTool::Reconstruct,
            Some("freeze") => LiquifyTool::Freeze,
            Some("thaw") => LiquifyTool::Thaw,
            _ => return Err(TOOL_REFUSAL.into()),
        };
        let number = |name: &str, default: Option<f64>, lo: f64, hi: f64| {
            match stroke.get(name) {
                None => default,
                Some(v) => v.as_f64(),
            }
            .filter(|n| n.is_finite() && (lo..=hi).contains(n))
            .ok_or_else(|| format!("liquify {name} must be finite in [{lo},{hi}]"))
        };
        let size = number("size", None, 1.0, 5000.0)?;
        let strength = number("strength", Some(0.5), 0.0, 1.0)?;
        let density = number("density", Some(0.5), 0.0, 1.0)?;
        let continued = match stroke.get("continued") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or("liquify continued must be true or false")?,
        };
        let pairs = stroke
            .get("points")
            .and_then(Value::as_array)
            .filter(|pairs| (1..=MAX_STROKE_POINTS).contains(&pairs.len()))
            .ok_or(POINTS_REFUSAL)?;
        total += pairs.len();
        if total > MAX_POINTS {
            return Err("liquify strokes hold at most 131072 points in all".into());
        }
        let mut points = Vec::with_capacity(pairs.len());
        for pair in pairs {
            let pair = pair
                .as_array()
                .filter(|pair| pair.len() == 2)
                .ok_or(POINTS_REFUSAL)?;
            let coordinate = |v: &Value| {
                v.as_f64()
                    .filter(|n| n.is_finite() && n.abs() <= 1_000_000.0)
                    .ok_or(POINTS_REFUSAL)
            };
            points.push([coordinate(&pair[0])?, coordinate(&pair[1])?]);
        }
        strokes.push(LiquifyStroke {
            tool,
            size,
            strength,
            density,
            continued,
            points,
        });
    }
    Ok(strokes)
}

/// The node grid of a plane (module doc, THE FIELD).
#[derive(Clone, Copy)]
struct Grid {
    cell: u32,
    columns: u32,
    rows: u32,
}

impl Grid {
    fn of(dimensions: (u32, u32)) -> Self {
        let (w, h) = dimensions;
        let cell = w.max(h).div_ceil(FIELD_MAX_SIDE).max(1);
        Self {
            cell,
            columns: w.div_ceil(cell),
            rows: h.div_ceil(cell),
        }
    }

    fn nodes(self) -> u64 {
        u64::from(self.columns) * u64::from(self.rows)
    }

    /// `(r, step)` of a brush of diameter `size` (module doc, A STROKE).
    fn brush(self, size: f64) -> (f64, f64) {
        let cell = f64::from(self.cell);
        let radius = (size / 2.0).max(1.5 * cell);
        (radius, cell.max(radius / 4.0))
    }

    /// The summed dab boxes of `stroke`: its dab count times `min(b,
    /// columns)·min(b, rows)`, `b = 2·⌈r / cell⌉ + 1` the most nodes a dab's
    /// disc spans along an axis. Saturating.
    fn work(self, stroke: &LiquifyStroke) -> u64 {
        let (radius, step) = self.brush(stroke.size);
        let span = 2 * (radius / f64::from(self.cell)).ceil() as u64 + 1;
        let area = span.min(u64::from(self.columns)) * span.min(u64::from(self.rows));
        let mut dabs = u64::from(!stroke.continued);
        for pair in stroke.points.windows(2) {
            dabs = dabs.saturating_add(segment_dabs(length(pair[0], pair[1]), step));
        }
        dabs.saturating_mul(area)
    }
}

fn length(from: [f64; 2], to: [f64; 2]) -> f64 {
    (to[0] - from[0]).hypot(to[1] - from[1])
}

/// The dabs of a segment of `length`: `⌈length / step⌉`, and one for a dwell.
fn segment_dabs(length: f64, step: f64) -> u64 {
    if length == 0.0 {
        1
    } else {
        ((length / step).ceil() as u64).max(1)
    }
}

/// The scratch bytes of [`plane`] over a plane of `dimensions`: the output
/// plane and the field. `Err`: dimensions outside the pixel limit, or
/// `strokes` whose summed dab boxes pass [`MAX_WORK`] (the 1-billion
/// sentence). It makes no other refusal and allocates nothing.
///
/// The request is OUTSIDE this plan: `strokes` itself (16 bytes a point, 2 MiB
/// at [`MAX_POINTS`]) and, in a caller that parsed it from JSON and keeps the
/// parsed value through the run (`dr_image_filter_op`, a selected edit's
/// `params`), about 96 bytes a point of `serde_json` values (12.6 MB at
/// [`MAX_POINTS`]) beside the text (at most [`MAX_REQUEST_BYTES`]). The count
/// caps bound all of it near 23 MB whatever the picture's size, and no plan
/// charges it; the `2 * BOOKKEEPING_BYTES` here is the kernel's own small
/// allocations, not the request.
pub(crate) fn plan(dimensions: (u32, u32), strokes: &[LiquifyStroke]) -> Result<u64, String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    let grid = Grid::of(dimensions);
    let mut work = 0_u64;
    for stroke in strokes {
        work = work.saturating_add(grid.work(stroke));
        if work > MAX_WORK {
            return Err(WORK_REFUSAL.into());
        }
    }
    Ok(n * PLANE_BYTES_PER_PIXEL + grid.nodes() * BYTES_PER_NODE + 2 * BOOKKEEPING_BYTES)
}

/// `strokes` applied to the identity field of a plane of `dimensions`, and
/// the whole plane rendered through it as straight samples (colour clamped to
/// 0…1). `None`: the field is still the identity, so nothing would change.
/// Refuses as [`plan`] does, before any allocation. Progress: the strokes over
/// 0…0.3, the render over 0.3…1. The 8- and 16-bit form: [`plane_in`] at
/// `SampleRange::Unit`, with the document's `linear` transfer.
pub(crate) fn plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    strokes: &[LiquifyStroke],
    linear: Option<&dyn LinearUnit>,
    job: JobContext<'_>,
) -> Result<Option<Vec<[f32; 4]>>, String> {
    plane_in(read, dimensions, strokes, &SampleRange::Unit, linear, job)
}

/// [`plane`] in `range` ([`LiquifyField::render`] says what it and `linear`
/// change). The field, the strokes and the taps are the same at every depth.
pub(crate) fn plane_in(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    strokes: &[LiquifyStroke],
    range: &SampleRange,
    linear: Option<&dyn LinearUnit>,
    job: JobContext<'_>,
) -> Result<Option<Vec<[f32; 4]>>, String> {
    plan(dimensions, strokes)?;
    let mut field = LiquifyField::new(dimensions)?;
    field.apply(strokes, job.range(0.0, 0.3))?;
    if field.is_identity() {
        return Ok(None);
    }
    field
        .render(
            read,
            dimensions,
            (0, 0, dimensions.0, dimensions.1),
            range,
            linear,
            job.range(0.3, 1.0),
        )
        .map(Some)
}

/// A plane's displacement and freeze field with the strokes applied so far
/// (module doc). It carries the summed work of those strokes, so a field built
/// one stroke at a time and one built from the whole list refuse at the same
/// stroke.
#[derive(Clone)]
pub struct LiquifyField {
    dimensions: (u32, u32),
    grid: Grid,
    displacement: Vec<[f32; 2]>,
    freeze: Vec<f32>,
    work: u64,
}

impl LiquifyField {
    /// The identity field of a plane of `dimensions`. `Err`: dimensions
    /// outside the pixel limit, or no memory for the nodes.
    pub(crate) fn new(dimensions: (u32, u32)) -> Result<Self, String> {
        pixel_count(dimensions.0, dimensions.1)?;
        let grid = Grid::of(dimensions);
        let nodes = grid.nodes() as usize;
        let mut displacement = reserved_vec(nodes)?;
        displacement.resize(nodes, [0.0; 2]);
        let mut freeze = reserved_vec(nodes)?;
        freeze.resize(nodes, 0.0);
        Ok(Self {
            dimensions,
            grid,
            displacement,
            freeze,
            work: 0,
        })
    }

    /// Applies `strokes` in order, IN PLACE (the field is the caller's own
    /// value; the FFI clones first). `Err`: a stroke that would take the
    /// field's summed work past [`MAX_WORK`], refused before that stroke
    /// changes anything (earlier strokes of the list stay applied), or a
    /// cancelled job.
    pub(crate) fn apply(
        &mut self,
        strokes: &[LiquifyStroke],
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let total: u64 = strokes
            .iter()
            .fold(0, |sum, s| sum.saturating_add(self.grid.work(s)));
        let mut done = 0_u64;
        let mut scratch = Vec::new();
        let mut polled = 0_usize;
        for stroke in strokes {
            let work = self.grid.work(stroke);
            let sum = self.work.saturating_add(work);
            if sum > MAX_WORK {
                return Err(WORK_REFUSAL.into());
            }
            self.work = sum;
            let (radius, step) = self.grid.brush(stroke.size);
            let segments = stroke.points.len().max(2) - 1;
            if !stroke.continued {
                let still = [0.0; 2];
                self.dab(
                    stroke,
                    radius,
                    stroke.points[0],
                    still,
                    1.0,
                    &mut scratch,
                    &mut polled,
                    job,
                )?;
            }
            for (index, pair) in stroke.points.windows(2).enumerate() {
                let (from, to) = (pair[0], pair[1]);
                let reach = length(from, to);
                if reach == 0.0 {
                    self.dab(
                        stroke,
                        radius,
                        to,
                        [0.0; 2],
                        1.0,
                        &mut scratch,
                        &mut polled,
                        job,
                    )?;
                } else {
                    let count = segment_dabs(reach, step);
                    let k = count as f64;
                    let motion = [(to[0] - from[0]) / k, (to[1] - from[1]) / k];
                    let weight = reach / (k * step);
                    for j in 1..=count {
                        let along = j as f64 / k;
                        let centre = [
                            from[0] + (to[0] - from[0]) * along,
                            from[1] + (to[1] - from[1]) * along,
                        ];
                        self.dab(
                            stroke,
                            radius,
                            centre,
                            motion,
                            weight,
                            &mut scratch,
                            &mut polled,
                            job,
                        )?;
                    }
                }
                if total > 0 {
                    let part = work as f64 * (index + 1) as f64 / segments as f64;
                    job.report((done as f64 + part) / total as f64, 6)?;
                }
            }
            done = done.saturating_add(work);
        }
        job.check()
    }

    /// One dab (module doc, A DAB). `scratch` is the reused copy of the box a
    /// moving tool rewrites; `polled` counts node rows across the whole
    /// `apply`, and every 64th polls the job, so a cancel waits for at most
    /// 64 rows of nodes whatever the brush size.
    #[allow(clippy::too_many_arguments)]
    fn dab(
        &mut self,
        stroke: &LiquifyStroke,
        radius: f64,
        centre: [f64; 2],
        motion: [f64; 2],
        weight: f64,
        scratch: &mut Vec<[f32; 2]>,
        polled: &mut usize,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let cell = f64::from(self.grid.cell);
        // The nodes whose centre can lie within `radius` of `centre`, clamped
        // to the grid. `as i64` saturates, and the clamp follows.
        let span = |c: f64, count: u32| -> Option<(usize, usize)> {
            let lo = (((c - radius) / cell - 0.5).ceil() as i64).max(0);
            let hi = (((c + radius) / cell - 0.5).floor() as i64).min(i64::from(count) - 1);
            (lo <= hi).then_some((lo as usize, hi as usize))
        };
        let (Some((i0, i1)), Some((j0, j1))) = (
            span(centre[0], self.grid.columns),
            span(centre[1], self.grid.rows),
        ) else {
            return Ok(());
        };
        let columns = self.grid.columns as usize;
        let full = 0.9 * stroke.density;
        // `(index, u, strength·falloff)` of a node inside the disc.
        let reach = |i: usize, j: usize| -> Option<(usize, [f64; 2], f64)> {
            let u = [
                (i as f64 + 0.5) * cell - centre[0],
                (j as f64 + 0.5) * cell - centre[1],
            ];
            let t = u[0].hypot(u[1]) / radius;
            if t >= 1.0 {
                return None;
            }
            let falloff = if t <= full {
                1.0
            } else {
                let s = (1.0 - t) / (1.0 - full);
                s * s * (3.0 - 2.0 * s)
            };
            Some((j * columns + i, u, stroke.strength * falloff))
        };
        let poll = |polled: &mut usize| {
            *polled += 1;
            if *polled & 63 == 0 {
                job.check()
            } else {
                Ok(())
            }
        };
        match stroke.tool {
            LiquifyTool::Freeze | LiquifyTool::Thaw => {
                for j in j0..=j1 {
                    poll(polled)?;
                    for i in i0..=i1 {
                        let Some((at, _, amount)) = reach(i, j) else {
                            continue;
                        };
                        let step = (amount * weight).min(1.0);
                        let frozen = f64::from(self.freeze[at]);
                        let next = if stroke.tool == LiquifyTool::Freeze {
                            frozen + (1.0 - frozen) * step
                        } else {
                            frozen * (1.0 - step)
                        };
                        self.freeze[at] = next as f32;
                    }
                }
            }
            LiquifyTool::Reconstruct => {
                for j in j0..=j1 {
                    poll(polled)?;
                    for i in i0..=i1 {
                        let Some((at, _, amount)) = reach(i, j) else {
                            continue;
                        };
                        let a = amount * (1.0 - f64::from(self.freeze[at]));
                        let keep = 1.0 - (a * weight).min(1.0);
                        let d = self.displacement[at];
                        self.displacement[at] = [
                            (f64::from(d[0]) * keep) as f32,
                            (f64::from(d[1]) * keep) as f32,
                        ];
                    }
                }
            }
            LiquifyTool::ForwardWarp
            | LiquifyTool::Pucker
            | LiquifyTool::Bloat
            | LiquifyTool::TwirlClockwise
            | LiquifyTool::TwirlCounterclockwise => {
                // Every read is of the field before this dab, so the new
                // values of the box are collected first and stored after.
                let width = i1 - i0 + 1;
                scratch.clear();
                scratch
                    .try_reserve(width * (j1 - j0 + 1))
                    .map_err(|_| "could not allocate liquify scratch".to_string())?;
                for j in j0..=j1 {
                    poll(polled)?;
                    for i in i0..=i1 {
                        let at = j * columns + i;
                        let before = self.displacement[at];
                        let Some((_, u, amount)) = reach(i, j) else {
                            scratch.push(before);
                            continue;
                        };
                        let a = amount * (1.0 - f64::from(self.freeze[at]));
                        let e = match stroke.tool {
                            LiquifyTool::ForwardWarp => [-a * motion[0], -a * motion[1]],
                            LiquifyTool::Pucker => {
                                [a * weight * RATE * u[0], a * weight * RATE * u[1]]
                            }
                            LiquifyTool::Bloat => {
                                [-a * weight * RATE * u[0], -a * weight * RATE * u[1]]
                            }
                            _ => {
                                // Content turning clockwise on the y-down
                                // plane is sampled from the point the standard
                                // rotation by −θ reaches.
                                let theta = a * weight * TWIRL_RADIANS;
                                let (sin, cos) = if stroke.tool == LiquifyTool::TwirlClockwise {
                                    (-theta).sin_cos()
                                } else {
                                    theta.sin_cos()
                                };
                                [
                                    u[0] * cos - u[1] * sin - u[0],
                                    u[0] * sin + u[1] * cos - u[1],
                                ]
                            }
                        };
                        if e == [0.0; 2] {
                            scratch.push(before);
                            continue;
                        }
                        let point = [
                            (i as f64 + 0.5) * cell + e[0],
                            (j as f64 + 0.5) * cell + e[1],
                        ];
                        let d = self.displacement_at(point);
                        scratch.push([(d[0] + e[0]) as f32, (d[1] + e[1]) as f32]);
                    }
                }
                for (row, values) in scratch.chunks_exact(width).enumerate() {
                    let start = (j0 + row) * columns + i0;
                    self.displacement[start..start + width].copy_from_slice(values);
                }
            }
        }
        Ok(())
    }

    /// The four nodes around the plane point `p` and their bilinear weights,
    /// node indexes clamped at the borders.
    fn corners(&self, p: [f64; 2]) -> [(usize, f64); 4] {
        let cell = f64::from(self.grid.cell);
        let axis = |v: f64, count: u32| -> (usize, usize, f64) {
            let f = v / cell - 0.5;
            let base = f.floor();
            let last = i64::from(count) - 1;
            // `as i64` saturates for a point far outside the plane.
            let lo = (base as i64).clamp(0, last);
            let hi = (base as i64).saturating_add(1).clamp(0, last);
            (lo as usize, hi as usize, f - base)
        };
        let (x0, x1, tx) = axis(p[0], self.grid.columns);
        let (y0, y1, ty) = axis(p[1], self.grid.rows);
        let columns = self.grid.columns as usize;
        [
            (y0 * columns + x0, (1.0 - tx) * (1.0 - ty)),
            (y0 * columns + x1, tx * (1.0 - ty)),
            (y1 * columns + x0, (1.0 - tx) * ty),
            (y1 * columns + x1, tx * ty),
        ]
    }

    fn displacement_at(&self, p: [f64; 2]) -> [f64; 2] {
        let mut out = [0.0; 2];
        for (at, weight) in self.corners(p) {
            let d = self.displacement[at];
            out[0] += weight * f64::from(d[0]);
            out[1] += weight * f64::from(d[1]);
        }
        out
    }

    /// Whether every node's displacement is zero: a render would return its
    /// source. The freeze does not count; it changes no pixel by itself.
    pub(crate) fn is_identity(&self) -> bool {
        self.displacement.iter().all(|d| *d == [0.0; 2])
    }

    /// `rect` (`x, y, width, height`) of a `source`-sized picture of the plane
    /// rendered through the field (module doc, THE RENDER): `width·height`
    /// straight samples, row-major, colour clamped to 0…1 at
    /// `SampleRange::Unit`. In a float layer (`Extended`) Catmull-Rom's
    /// negative lobes are bounded by the layer's visible range instead
    /// (`SampleRange::resampled`, D3 in `float_document`'s module doc) and
    /// the transparent floor is 2^-24. `read(i)` is the
    /// picture's straight pixel `i`. The picture may be any size: a pixel
    /// centre is scaled to the plane to read `D`, and the displaced point is
    /// scaled back. `Err`: an empty rect or one not inside the picture, or a
    /// cancelled job. Progress 0…1 over the rect's rows. `linear` (a
    /// linear-light 8- or 16-bit document's transfer, `Unit` only) decodes
    /// each tap's RGB before the premultiplied sum and encodes the clamped
    /// result once; an untouched pixel is still `read(i)`.
    pub(crate) fn render(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        source: (u32, u32),
        rect: (u32, u32, u32, u32),
        range: &SampleRange,
        linear: Option<&dyn LinearUnit>,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        let (sw, sh) = source;
        let (rx, ry, rw, rh) = rect;
        if rw == 0
            || rh == 0
            || u64::from(rx) + u64::from(rw) > u64::from(sw)
            || u64::from(ry) + u64::from(rh) > u64::from(sh)
        {
            return Err(RECT_REFUSAL.into());
        }
        let scale = [
            f64::from(self.dimensions.0) / f64::from(sw),
            f64::from(self.dimensions.1) / f64::from(sh),
        ];
        let count = rw as usize * rh as usize;
        let mut out = reserved_vec(count)?;
        out.resize(count, [0.0_f32; 4]);
        // Below half a 16-bit code the alpha quantizes to transparent at
        // either depth, and dividing a colour sum by it would invent a colour
        // nobody can see (`ops_spatial::box_plane`'s floor). A float layer's
        // floor is 2^-24 (`SampleRange::alpha_floor`).
        let floor = range.alpha_floor();
        // Set by the map for the pixel being walked and read by the write
        // that follows it: `map_into` calls them in that order, pixel by pixel.
        let untouched = Cell::new(false);
        map_into(
            source,
            (rw, rh),
            Kernel::Bicubic,
            |x, y, _| {
                let centre = [f64::from(rx) + x, f64::from(ry) + y];
                let d = self.displacement_at([centre[0] * scale[0], centre[1] * scale[1]]);
                untouched.set(d == [0.0; 2]);
                if untouched.get() {
                    return Ok(centre);
                }
                // Clamped to the pixel centres, so a point beyond the picture
                // samples the edge pixel itself.
                Ok([
                    (centre[0] + d[0] / scale[0]).clamp(0.5, f64::from(sw) - 0.5),
                    (centre[1] + d[1] / scale[1]).clamp(0.5, f64::from(sh) - 0.5),
                ])
            },
            &read,
            // Premultiplied, as `doc_warp::render_pixels` accumulates; the
            // fifth sum is the weight of the taps that were inside.
            |acc: &mut [f64; 5], p: [f32; 4], weight| {
                let rgb = match linear {
                    Some(transfer) => transfer.linear([p[0], p[1], p[2]]),
                    None => [p[0], p[1], p[2]],
                };
                let alpha = weight * f64::from(p[3]);
                for c in 0..3 {
                    acc[c] += alpha * f64::from(rgb[c]);
                }
                acc[3] += alpha;
                acc[4] += weight;
            },
            |acc| {
                if acc[4] <= 0.0 || acc[3] / acc[4] < floor {
                    return [0.0_f32; 4];
                }
                let rgb = [0, 1, 2].map(|c| range.resampled(c, acc[c] / acc[3]) as f32);
                let rgb = match linear {
                    Some(transfer) => transfer.encoded(rgb),
                    None => rgb,
                };
                [rgb[0], rgb[1], rgb[2], (acc[3] / acc[4]).min(1.0) as f32]
            },
            |i, value| {
                out[i] = if untouched.get() {
                    let (x, y) = (i % rw as usize, i / rw as usize);
                    read((ry as usize + y) * sw as usize + rx as usize + x)
                } else {
                    value
                };
            },
            job,
        )?;
        Ok(out)
    }

    /// The byte adapter of [`Self::render`] for the workspace's preview:
    /// `image` (a picture of the plane at any size) through the field. With no
    /// `previous`, the whole picture (`rect` is not read). With one (the same
    /// size as `image`), a copy of it with `rect` re-rendered from `image`.
    /// Samples are quantized as `ops_spatial::filter_op` quantizes its own.
    /// `Err`: a `previous` of another size, or `render`'s rect refusal.
    pub(crate) fn render_rgba8(
        &self,
        image: &RgbaImage,
        previous: Option<&RgbaImage>,
        rect: (u32, u32, u32, u32),
    ) -> Result<RgbaImage, String> {
        let (w, h) = image.dimensions();
        let (rx, ry, rw, rh) = if previous.is_some() {
            rect
        } else {
            (0, 0, w, h)
        };
        if rw == 0
            || rh == 0
            || u64::from(rx) + u64::from(rw) > u64::from(w)
            || u64::from(ry) + u64::from(rh) > u64::from(h)
        {
            return Err(RECT_REFUSAL.into());
        }
        let raw = image.as_raw().as_chunks::<4>().0;
        let mut out: Vec<u8> = reserved_vec(raw.len() * 4)?;
        match previous {
            Some(previous) if previous.dimensions() != (w, h) => {
                return Err("liquify previous image must match the image size".into());
            }
            Some(previous) => out.extend_from_slice(previous.as_raw()),
            None => out.resize(raw.len() * 4, 0),
        }
        let unit = |i: usize| raw[i].map(|v| f32::from(v) / 255.0);
        let mut top = ry;
        while top < ry + rh {
            let rows = RENDER_BAND_ROWS.min(ry + rh - top);
            let band = self.render(
                unit,
                (w, h),
                (rx, top, rw, rows),
                &SampleRange::Unit,
                None,
                JobContext::NONE,
            )?;
            for (row, samples) in band.chunks_exact(rw as usize).enumerate() {
                let start = ((top as usize + row) * w as usize + rx as usize) * 4;
                for (pixel, sample) in out[start..start + rw as usize * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(samples)
                {
                    if sample.iter().any(|v| !v.is_finite()) {
                        return Err("filter produced an invalid component".into());
                    }
                    *pixel = sample.map(|v| u8::quantize(v * 255.0));
                }
            }
            top += rows;
        }
        RgbaImage::from_raw(w, h, out).ok_or_else(|| "invalid liquify image dimensions".into())
    }

    /// The freeze as coverage 0…255, sampled at the pixel centres of a
    /// `dimensions`-sized picture of the plane, row-major. `Err`: dimensions
    /// outside the pixel limit, or no memory.
    pub(crate) fn freeze(&self, dimensions: (u32, u32)) -> Result<Vec<u8>, String> {
        let count = pixel_count(dimensions.0, dimensions.1)?;
        let scale = [
            f64::from(self.dimensions.0) / f64::from(dimensions.0),
            f64::from(self.dimensions.1) / f64::from(dimensions.1),
        ];
        let mut out = reserved_vec(count)?;
        for y in 0..dimensions.1 {
            for x in 0..dimensions.0 {
                let p = [
                    (f64::from(x) + 0.5) * scale[0],
                    (f64::from(y) + 0.5) * scale[1],
                ];
                let frozen: f64 = self
                    .corners(p)
                    .iter()
                    .map(|&(at, weight)| weight * f64::from(self.freeze[at]))
                    .sum();
                out.push((frozen * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
        Ok(out)
    }
}
