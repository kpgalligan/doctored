//! Phase 13A-4a's GUIDE GEOMETRY for the regional compositor (13A-4b-i: per
//! level): where a guide-reading Shadows/Highlights is built, how far its plane
//! reaches, and the index arithmetic the staged window loops
//! (`doc_regional_guide`, `precision_regional_guide`) share. No pixel math
//! lives here: the guide is the reference's own (`Adjustment::guide_with_job`
//! / `PrecisionAdjustment::guide`, both `adjust_guide::build`), the sample the
//! reference's own (`doc::adjust_backdrop_sample` /
//! `precision_render::adjust_backdrop_sample16`) with the guide value in place
//! of `0.0`.
//!
//! EXACTNESS (tolerance: EXACT at 8 and 16 bits, declared before measuring).
//! `adjust_guide::build` builds the guide CELL-WISE on a lattice fixed in
//! accumulator coordinates — cell `c` is pixels `[c·f, (c+1)·f)` — so a plane
//! whose origin is a multiple of `f` holds exactly the reference's cells over
//! its extent, and a plane whose end is the canvas end holds the reference's
//! ragged last cell. The cells are then blurred by `plane_blur` — at 8 bits
//! `style_render::blur_plane_with_job`, which is BIT-IDENTICAL on a sub-plane whose
//! origin sits on the [`SUB_PLANE_ALIGN`] lattice (in cells) and whose reads
//! lie at least `blur_reach` inside it (`style_render`'s module doc); at typed
//! depth `gaussian_plane::feather`, a direct clamped convolution whose half
//! width is at most `blur_reach` — and read by `GuidePlane::at`, which
//! interpolates between the two cells around a pixel (±1 cell). A plane over
//! [`GuideGeometry::plane_region`]`(R)` — `R` grown by `f · reach_cells`
//! (`reach_cells = blur_reach(grid_sigma) + 2`: the blur's reach plus the
//! bilinear read's cell and one for the floor), its origin rounded DOWN and
//! its end UP to multiples of `f · SUB_PLANE_ALIGN`, clamped to the canvas —
//! therefore yields the reference's guide value bit for bit at every sample
//! of `R`, provided the plane holds the reference's accumulator exactly,
//! which the regional per-sample evaluation at divisor 1 guarantees. At a
//! canvas edge the plane's edge IS the canvas edge (the reference's own
//! clamp / zero extension), never anywhere else: the BORDER POLICY
//! [`BORDER_POLICY`]. The factors are powers of two, so the plane-local cell
//! coordinate `(x − o + 0.5)/f − 0.5` differs from the canvas one by the
//! integer `o/f` exactly in f32.
//!
//! STAGES. With `m` guided rows at the root, planes are chosen from the LAST
//! guide backwards: `R_m` is the request window and `R_{k−1} =
//! plane_region_k(R_k)`, so `stages[k−1]` (guide `k`) is built over `R_{k−1}`
//! and read over `R_k`. A stage's `layer` is a LAYER index (the byte stack
//! index / the typed postorder record index — the plan row's `index`), never
//! a unit position; only `ResolvedLevel::path_of_layer` (byte) /
//! `guided_paths` (typed) maps it to a cursor (13A-4b-ii-a: unit positions
//! down through pass-through children; 13A-4b-ii-b-1: ending, for a clip
//! member, in its step position).
//!
//! LEVELS (13A-4b-i). An ISOLATED group's level is staged the same way over
//! its BUFFER — the accumulator the reference builds its guide from: at 8
//! bits `rendered_group`'s buffer over the group's rect (the guide read at
//! buffer-local coordinates, so the lattice is anchored at the rect's origin
//! and the plane's clamp edge is the rect's), at typed depth a canvas-sized
//! plane. Everything above holds with `buffer` in place of the canvas.
//! [`plan_levels`] walks the walk's [`LevelRecord`] tree: a level's `R_m`
//! is the region its unit is evaluated over in the enclosing level (a stage
//! plane below the enclosing level's last guide, else that level's `R_m`)
//! intersected with its buffer, its last stage a divisor-1 plane over `R_m`
//! that is HELD (only the root's samples the output). Levels are emitted
//! pre-order and only when they hold a guided row; a level under a
//! MATERIALIZED group ([`Enclosure::Materialized`]) is never recorded.
//!
//! PASS-THROUGH (13A-4b-ii-a). A pass-through group's children composite onto
//! the ENCLOSING accumulator (plain) or a copy of it (gated) — the same
//! buffer, origin and lattice — so a pass-through level is never a level of
//! its own: its record is ABSORBED into the enclosing Root / Isolated one
//! ([`LevelRecord::absorb`]), its guided rows become stages of that level
//! with the pass-through groups between the level and the row in
//! [`GuidedRow::through`] (outermost first), and its isolated children join
//! the enclosing level's children. Where a pass-through group on a stage's
//! path is `held` (gated, or a Shallow child reads the sample as its children
//! began) that sample is HELD as a plane ([`Holds`]) from the fill that
//! enters the group to the fill that finishes it; the plan charges one plane
//! over `R_0` per distinct held group of the level (scratch `16·|R_0|`, work
//! `|R_0|`, the fill), conservatively (a hold is never dropped early).
//!
//! CLIP UNITS (13A-4b-ii-b-1). A guided clip MEMBER of an unstyled base is
//! built from the unit's private buffer, which the reference allocates with
//! the ENCLOSING accumulator's size and origin (8 bits `doc.rs:579`, typed a
//! canvas-sized plane), so the level's lattice and clamp apply unchanged. Its
//! cursor ends in the member's step position; the stage plane holds the unit
//! buffer there. The sample as the unit began is HELD from the fill that
//! enters the unit to the fill that finishes it (one plane over `R_0` per
//! distinct unit, keyed by the base's index), and every later fill RE-RUNS
//! the unit from that held sample — so a member stage's `rows` count from the
//! unit's first row ([`ClipUnit::start`]). Since 13A-4b-ii-b-2 the base may
//! be STYLED: the unit buffer is the style's private buffer (the
//! styled-direct member loop or the routed unit), which the reference also
//! allocates with the enclosing accumulator's size and origin — typed, the
//! package's canvas-sized copy — so the same hold, re-run and row rule apply
//! (the 8-bit package, per strip, is refused).
//!
//! HALO. [`input_halo`] is the plan's top-level figure: a row evaluated below
//! guides is evaluated over their planes, so its own halo is added to theirs.
//!
//! since 13A-4a, a GUIDED Shadows/Highlights at the ROOT at 8 and 16 bits, EXACT, through STAGES
//! (`regional_guide.rs`: `guide_geometry`, `GuideGeometry::plane_region` — the next stage's region
//! grown by the guide's reach and aligned outward to `factor · SUB_PLANE_ALIGN`, aligned relative
//! to and clamped by the level's BUFFER — the canvas at the root and at every typed level, the
//! group's rect at 8 bits (13A-4b-i) — so `adjust_guide::build`'s cell lattice, `plane_blur`'s
//! sub-plane identity and `GuidePlane::at`'s ±1-cell read make the guide bit-identical; `Enclosure`
//! / `LevelRecord` from the walks, `plan_levels` (the staged levels PRE-ORDER, their scratch and
//! work), `input_halo`, `PreparedGuide`): the drivers `doc_regional_guide::render_staged` /
//! `precision_regional_guide::render_staged` fill `P_0` at divisor 1 with the units below the first
//! guide (D5's Background split and its snapshot `S` held over `R_0`), build each guide from the
//! previous plane with the reference's own builder (`Adjustment::guide_with_job` →
//! `adjust_guide::tone_plane_with_job`; typed `PrecisionAdjustment::guide` under
//! `progress.job.range`), hand it to the unit (`ResolvedNode.guide` via `ResolvedLevel::set_guide`;
//! typed `Node.guide`) and evaluate the next units over the next plane or the output; `Stage.layer`
//! and the walk's `guided` are LAYER indices (byte `Node.at.idx`, typed record index) — only
//! `ResolvedLevel::path_of_layer` (byte) / `guided_paths` (typed) map a layer to a resolved cursor
//! path, because `resolve` drops `None` bases; the plan adds `"stages"` and `"border_policy":
//! "canvas_clamped"`, a guided row is `Spatial` with `max(mask pad, factor · (reach_cells +
//! SUB_PLANE_ALIGN))` and the plan's halo is `input_halo` (each row's own plus the halos of the
//! guides above it); a preview below a guided row still pays divisor-1 stages (stated); a guided
//! base over a non-empty run contributes nothing (its ordinary row: no stage, no guide halo). Since
//! 13A-4b-i a guided lone unit of an ISOLATED group's level is staged the same way before the root
//! loop (`stage_nested`: per-level stages over the group's buffer, the last plane HELD at divisor 1
//! — even for a preview — and read in place of the children through byte `Source::Staged` / typed
//! `Node.staged`; byte levels are looked up by group index, typed ones popped in pre-order), and
//! one anywhere under a MATERIALIZED group (`Enclosure::Materialized`, sticky) is rendered by the
//! reference and never staged; since 13A-4b-ii-a one directly inside a PASS-THROUGH group (plain or
//! gated, nested) is staged by the ENCLOSING level: the walks ABSORB a pass-through level's record
//! (`LevelRecord::absorb`, each stage's `through` naming the groups, outermost first), the drivers
//! reach the guided unit by a CURSOR (byte `ResolvedLevel::path_of_layer` / typed `guided_paths`)
//! and evaluate each fill from one cursor to the next (`sample_between`), and the sample as each
//! pass-through group's children began is HELD as a plane (`regional_guide::Holds`, at 8 bits only
//! where `pass_through_holds` says it is read, typed always) from the fill that enters the group to
//! the one that finishes it (`gate_source_into` / `gate_pass_through_sample16`), each held group
//! charged one more plane over `R_0`; since 13A-4b-ii-b-1 as a CLIP MEMBER of an unstyled base
//! (materialized subtrees rendered by the reference): its cursor ends in the member's step, the
//! sample as the unit began is HELD from the fill that enters the unit and every later fill RE-RUNS
//! the unit from it (`ResolvedUnit::sample_clip_until` /
//! `precision_regional_tree::clip_unit_sample`), each distinct clip unit charged one more plane
//! over `R_0` and a member stage's rows counted from the unit's first row; since 13A-4b-ii-b-2
//! under a STYLED base too — the staged cursor's member index is a STOP inside the style's own
//! member loop (byte `styled_sample` / `direct_sample` / `unit_pass_sample`, typed
//! `styled_sample16` / `direct_sample16` / `unit_pass_sample16`, the private buffer left in the
//! sample before `composite_source_into` / `K` and the Interior contributions; the typed package's
//! one canvas-sized copy holds it), reached by `sample_clip_until` / `clip_unit_sample` dispatching
//! a styled base — while the 8-bit PACKAGE (effects below opacity 1, not Dissolve; members per
//! strip) refuses "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects
//! below opacity 1)" (owner 13B).

use std::ops::Range;

use crate::adjust_guide::GuidePlane;
use crate::regional::RegionRequest;
pub(crate) use crate::regional_plan::intersect;
use crate::regional_plan::PixelRegion;
pub(crate) use crate::regional_plan::{input_halo, plan_levels};
use crate::style_render::{
    blur_reach, downsample_factor, reduced_sigma, sigma_for_size, SUB_PLANE_ALIGN,
};

/// The plan's `border_policy`: a guide plane meets the reference's own edge
/// handling at the canvas edge and nowhere else.
pub(crate) const BORDER_POLICY: &str = "canvas_clamped";

/// One guide's lattice: its cell size in pixels (`downsample_factor` of the
/// guide's sigma, 1 = the guide is per pixel) and how many cells around a
/// read the blur and the bilinear read reach.
#[derive(Clone, Copy)]
pub(crate) struct GuideGeometry {
    pub(crate) factor: u32,
    pub(crate) reach_cells: u32,
}

/// The geometry `adjust_guide::build` uses at `radius`: `sigma_for_size` →
/// `downsample_factor`; the grid sigma is `sigma` at factor 1, else
/// `reduced_sigma`; `reach_cells = blur_reach(grid_sigma) + 2`.
pub(crate) fn guide_geometry(radius: f32) -> GuideGeometry {
    let sigma = sigma_for_size(radius);
    let factor = downsample_factor(sigma);
    let grid_sigma = if factor == 1 {
        sigma
    } else {
        reduced_sigma(sigma, factor)
    };
    GuideGeometry {
        factor,
        reach_cells: blur_reach(grid_sigma) + 2,
    }
}

impl GuideGeometry {
    /// The declared halo: `factor · (reach_cells + SUB_PLANE_ALIGN)`, which
    /// bounds one [`Self::plane_region`] step's growth on every side (the
    /// reach plus up to `SUB_PLANE_ALIGN − 1` cells of alignment).
    pub(crate) fn halo_pixels(self) -> u32 {
        self.factor
            .saturating_mul(self.reach_cells.saturating_add(SUB_PLANE_ALIGN))
    }

    /// The plane the guide is built over so that it is exact over `inner`
    /// (module doc): `inner` grown by `factor · reach_cells`, its origin
    /// rounded DOWN and its end UP to multiples of `factor · SUB_PLANE_ALIGN`
    /// RELATIVE TO `buffer`'s origin — the lattice of the accumulator the
    /// reference builds the guide from (13A-4b-i) — and clamped to `buffer`.
    /// At the root `buffer` is the canvas rect (13A-4a's formula).
    pub(crate) fn plane_region(self, inner: PixelRegion, buffer: PixelRegion) -> PixelRegion {
        let grow = u64::from(self.factor) * u64::from(self.reach_cells);
        let lattice = u64::from(self.factor) * u64::from(SUB_PLANE_ALIGN);
        let (bx, by) = (u64::from(buffer.x), u64::from(buffer.y));
        let (bx1, by1) = (bx + u64::from(buffer.width), by + u64::from(buffer.height));
        let (ix, iy) = (u64::from(inner.x), u64::from(inner.y));
        let x0 = bx + ix.saturating_sub(bx).saturating_sub(grow) / lattice * lattice;
        let y0 = by + iy.saturating_sub(by).saturating_sub(grow) / lattice * lattice;
        let x1 = (bx
            + (ix + u64::from(inner.width) + grow)
                .saturating_sub(bx)
                .div_ceil(lattice)
                * lattice)
            .min(bx1);
        let y1 = (by
            + (iy + u64::from(inner.height) + grow)
                .saturating_sub(by)
                .div_ceil(lattice)
                * lattice)
            .min(by1);
        PixelRegion {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        }
    }
}

/// One guided row's stage: the guide of plan row `layer` (a LAYER index,
/// never a unit position), built over `plane` with `geometry`; `rows` is the
/// number of plan rows from this guided row (inclusive) — or, for a CLIP
/// MEMBER's stage, from its unit's FIRST row (`clip_unit.start`: the fill
/// re-runs the unit from its base, 13A-4b-ii-b-1) — to the next guided row
/// of its level (exclusive): the rows evaluated over the next stage's plane
/// (or the level's last region); `through` the pass-through groups between
/// the level and the row, outermost first (13A-4b-ii-a); `clip_unit` the
/// member's unit (13A-4b-ii-b-1), `None` for a lone row.
pub(crate) struct Stage {
    pub(crate) layer: usize,
    pub(crate) plane: PixelRegion,
    pub(crate) geometry: GuideGeometry,
    pub(crate) rows: u64,
    pub(crate) through: Vec<Through>,
    pub(crate) clip_unit: Option<ClipUnit>,
}

/// One pass-through group between a staged level and a guided row
/// (13A-4b-ii-a): its index (as `Stage.layer` is) and whether the sample as
/// its children began is HELD as a plane — byte `pass_through_holds` (the
/// gate, or a Shallow child), typed always (the typed arm always copies).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Through {
    pub(crate) group: usize,
    pub(crate) held: bool,
}

/// A guided CLIP MEMBER's unit (13A-4b-ii-b-1): its base's LAYER index and
/// the unit's FIRST plan-row index — the base's subtree start (byte
/// `Unit.start`, typed `starts[i]`; the base's own index for a leaf).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ClipUnit {
    pub(crate) base: usize,
    pub(crate) start: usize,
}

/// One staged guided row of a level: its LAYER index, its radius,
/// `through`: the pass-through groups between the level and the row,
/// OUTERMOST first (empty for a level's own row), and `clip_unit`: the unit
/// of a guided clip MEMBER (13A-4b-ii-b-1), `None` for a lone row.
pub(crate) struct GuidedRow {
    pub(crate) layer: usize,
    pub(crate) radius: f32,
    pub(crate) through: Vec<Through>,
    pub(crate) clip_unit: Option<ClipUnit>,
}

/// Where a LEVEL sits (13A-4b-i), decided by the walks before the level is
/// walked: the root; an isolated group's children (a buffer the guide can be
/// staged over); a pass-through group's children (13A-4b-ii's); or anywhere
/// below a MATERIALIZED group — sticky down every descendant level — whose
/// subtree the reference renders itself, so nothing in it is staged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Enclosure {
    Root,
    Isolated,
    PassThrough,
    Materialized,
}

/// One level as the walk recorded it for [`plan_levels`]. `group` is the
/// group's index as `Stage.layer` is (byte layer / typed record; `None` at
/// the root); `range` the LAYER indices of the level (byte `start..idx`,
/// typed `first..index`, the whole stack at the root); `buffer` the level's
/// clamp rect — a byte isolated group's `Node.rect` (`None` when empty: its
/// subtree has no stages), the ENCLOSING level's for a pass-through level,
/// the canvas at the root and at every typed level; `guided` the staged
/// guided rows of THIS level, pass-through children's absorbed (row order);
/// `children` each Isolated child level in WALK order with the number of
/// this level's guided rows strictly below the child's unit. A PassThrough
/// child is absorbed ([`Self::absorb`], 13A-4b-ii-a); a Materialized child is
/// never recorded.
pub(crate) struct LevelRecord {
    pub(crate) group: Option<usize>,
    pub(crate) range: Range<usize>,
    pub(crate) buffer: Option<PixelRegion>,
    pub(crate) guided: Vec<GuidedRow>,
    pub(crate) children: Vec<(usize, LevelRecord)>,
}

impl LevelRecord {
    /// Merges a PASS-THROUGH child level `child` of group `group` into this
    /// (Root / Isolated / PassThrough) record: `children` appended with
    /// `below + self.guided.len()` (taken before the extend); `guided`
    /// appended with `Through { group, held }` INSERTED AT 0 of each row's
    /// `through`.
    pub(crate) fn absorb(&mut self, group: usize, held: bool, child: LevelRecord) {
        let below = self.guided.len();
        self.children.extend(
            child
                .children
                .into_iter()
                .map(|(count, level)| (count + below, level)),
        );
        self.guided.extend(child.guided.into_iter().map(|mut row| {
            row.through.insert(0, Through { group, held });
            row
        }));
    }
}

/// One STAGED level of the plan: the level (`None` at the root, else the
/// group's index), its clamp rect, its last region `target` (`R_m`: the
/// request window at the root, the held final plane below it), the rows
/// evaluated over `R_0` and its stages (never empty).
pub(crate) struct StagedLevel {
    pub(crate) level: Option<usize>,
    pub(crate) buffer: PixelRegion,
    pub(crate) target: PixelRegion,
    pub(crate) rows_0: u64,
    pub(crate) stages: Vec<Stage>,
}

/// The accumulator as pass-through unit `path` (a cursor prefix) began, over
/// the region of the FILL it was entered in (13A-4b-ii-a).
pub(crate) struct Held {
    pub(crate) path: Vec<usize>,
    pub(crate) region: PixelRegion,
    pub(crate) plane: Vec<[f32; 4]>,
}

/// Every hold of one staged render, alive for the whole call.
pub(crate) struct Holds(pub(crate) Vec<Held>);

impl Holds {
    /// `plane` is the caller's chunk-filled zero plane of `pixels(region)`
    /// samples (byte `plane(n, job)`, typed `zero_plane(n, progress)` — each
    /// polls every 4096 samples); `Holds` allocates and fills nothing itself.
    pub(crate) fn enter(&mut self, path: &[usize], region: PixelRegion, plane: Vec<[f32; 4]>) {
        let held = Held {
            path: path.to_vec(),
            region,
            plane,
        };
        match self.0.iter_mut().find(|h| h.path == path) {
            Some(slot) => *slot = held,
            None => self.0.push(held),
        }
    }

    /// Records the sample at canvas pixel `canvas_xy` (inside the hold's
    /// region) as `path` began; a no-op when `path` is not held.
    pub(crate) fn write(&mut self, path: &[usize], canvas_xy: (i64, i64), value: [f32; 4]) {
        if let Some(held) = self.0.iter_mut().find(|h| h.path == path) {
            held.plane[index_in(held.region, canvas_xy)] = value;
        }
    }

    /// The held sample of `path` at canvas pixel `canvas_xy`; `None` when
    /// `path` is not held.
    pub(crate) fn at(&self, path: &[usize], canvas_xy: (i64, i64)) -> Option<[f32; 4]> {
        self.0
            .iter()
            .find(|h| h.path == path)
            .map(|held| held.plane[index_in(held.region, canvas_xy)])
    }
}

/// A nested staged level's held final plane over `region` (its `target`),
/// read in place of the group's children at every sample inside it.
pub(crate) struct StagedPlane {
    pub(crate) region: PixelRegion,
    pub(crate) plane: Vec<[f32; 4]>,
}

impl StagedPlane {
    /// The plane's sample at canvas pixel `canvas_xy`, which lies in `region`.
    pub(crate) fn at(&self, canvas_xy: (i64, i64)) -> [f32; 4] {
        self.plane[index_in(self.region, canvas_xy)]
    }
}

/// Where a staged level's last stage is evaluated: the OUTPUT at the
/// request's sampling (the root), or a divisor-1 plane over a region (a
/// nested level's held final plane).
pub(crate) enum Last {
    Output(RegionRequest),
    Region(PixelRegion),
}

/// A guide plane placed on the canvas: `plane` was built over the region
/// whose origin is `origin`, and is read at canvas coordinates.
pub(crate) struct PreparedGuide {
    plane: GuidePlane,
    origin: (i64, i64),
}

impl PreparedGuide {
    pub(crate) fn new(plane: GuidePlane, region: PixelRegion) -> Self {
        Self {
            plane,
            origin: (i64::from(region.x), i64::from(region.y)),
        }
    }

    /// The guide at canvas pixel `canvas_xy`, which lies inside the region
    /// the plane was built over (the drivers read it over the next stage's
    /// region only).
    pub(crate) fn at(&self, canvas_xy: (i64, i64)) -> f32 {
        debug_assert!(canvas_xy.0 >= self.origin.0 && canvas_xy.1 >= self.origin.1);
        self.plane.at(
            (canvas_xy.0 - self.origin.0) as u32,
            (canvas_xy.1 - self.origin.1) as u32,
        )
    }
}

/// The row-major index of canvas pixel `canvas_xy` in a plane over `region`.
pub(crate) fn index_in(region: PixelRegion, canvas_xy: (i64, i64)) -> usize {
    let x = (canvas_xy.0 - i64::from(region.x)) as usize;
    let y = (canvas_xy.1 - i64::from(region.y)) as usize;
    y * region.width as usize + x
}
