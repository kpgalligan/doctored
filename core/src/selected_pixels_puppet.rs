//! Puppet Warp of the selection's FLOAT: the geometry half of
//! `SelectedPixelsEdit::PuppetSelected`, Edit ▸ Puppet Warp with a
//! selection active. The float, the hole `a·(1−c)`, the source-over landing
//! and the single quantize are the selected-pixels walk
//! (`selected_pixels_walk`); this module says where the float lands and
//! what it looks like there, as `selected_pixels_transform` does for Free
//! Transform.
//!
//! The mesh is built over the float's pixels: alpha > 0 AND coverage > 0,
//! the same membership the mesh export takes with a canvas coverage
//! ([`float_mesh`]), and because the grid is anchored at the mask's own box
//! both build the same mesh. The pose is solved as for a layer, the landing
//! is the deformed mesh's box ∩ canvas, and the premultiplied f32 float
//! (`selected_pixels_transform::float_sample`) resamples through
//! `puppet_render::render` (Catmull-Rom, binary64 taps) before landing
//! through the walk's shared tail. The selection itself is unchanged.
use crate::job_control::JobContext;
use crate::precision_image::{admit, reserved_vec};
use crate::puppet_arap::{solve, PuppetSpec, SOLVER_BYTES};
use crate::puppet_mesh::{build, mesh_plan_bytes, MeshOptions, PuppetMesh};
use crate::puppet_render::{render, PoseMap, MAP_BYTES};
use crate::selected_pixels::{area, intersect, CanvasRect};

pub(crate) const EMPTY_AREA: &str = "The selection holds no pixels to puppet warp.";

/// The mesh of the selected opaque pixels of the layer standing at `layer`
/// (`opaque` in layer pixels), `coverage` canvas-sized u16, after admitting
/// `mesh_plan_bytes` over layer ∩ canvas.
#[allow(clippy::too_many_arguments)]
pub(crate) fn float_mesh(
    layer: CanvasRect,
    canvas: (u32, u32),
    coverage: &[u16],
    opaque: impl Fn(u32, u32) -> bool,
    options: &MeshOptions,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<PuppetMesh, String> {
    if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
        return Err("puppet warp coverage must be canvas-sized u16".into());
    }
    let Some(rect) = intersect(layer, (0, 0, canvas.0, canvas.1)) else {
        return Err(crate::puppet_mesh::empty_refusal(options.expansion));
    };
    admit(0, mesh_plan_bytes(rect, options)?, other, limit)?;
    let (dx, dy) = ((rect.0 - layer.0) as u32, (rect.1 - layer.1) as u32);
    build(
        rect,
        |x, y| {
            let at =
                (rect.1 as usize + y as usize) * canvas.0 as usize + rect.0 as usize + x as usize;
            coverage[at] > 0 && opaque(x + dx, y + dy)
        },
        options,
        job,
    )
}

/// The landing rect and the premultiplied samples resampled onto it.
pub(crate) type Landed = (CanvasRect, Vec<[f32; 4]>);

/// A resolved, non-identity Puppet Warp of the float.
#[derive(Clone, Debug)]
pub(crate) struct PuppetLanding {
    pub(crate) spec: PuppetSpec,
}

impl PuppetLanding {
    /// The plan's landing term over the layer's on-canvas rect `bound`: the
    /// whole canvas at 16 bytes a pixel (the deformed box is not known
    /// before the solve) plus [`Self::solve_scratch`].
    pub(crate) fn scratch(
        &self,
        canvas: (u32, u32),
        bound: Option<CanvasRect>,
    ) -> Result<u64, String> {
        Ok(16u64
            .saturating_mul(u64::from(canvas.0) * u64::from(canvas.1))
            .saturating_add(self.solve_scratch(bound)?))
    }

    /// The mesh over `bound` (the float lies inside it), the solver, and the
    /// [`PoseMap`] [`Self::resample`] builds.
    pub(crate) fn solve_scratch(&self, bound: Option<CanvasRect>) -> Result<u64, String> {
        let mesh = match bound {
            Some(rect) => mesh_plan_bytes(rect, &self.spec.options)?,
            None => 0,
        };
        Ok(mesh.saturating_add(SOLVER_BYTES).saturating_add(MAP_BYTES))
    }

    /// Mesh the float (`float`, canvas rect; `opaque` its membership in
    /// canvas pixels), solve, and resample the premultiplied `floated`
    /// samples onto the landing (deformed box ∩ canvas). `Ok(None)` when the
    /// landing misses the canvas.
    pub(crate) fn resample(
        &self,
        floated: &[[f32; 4]],
        float: CanvasRect,
        canvas: (u32, u32),
        opaque: impl Fn(i64, i64) -> bool,
        job: JobContext<'_>,
    ) -> Result<Option<Landed>, String> {
        let mesh = build(
            float,
            |x, y| opaque(float.0 + i64::from(x), float.1 + i64::from(y)),
            &self.spec.options,
            job,
        )?;
        let pose = solve(&mesh, &self.spec.pins, self.spec.mode, job)?;
        let map = PoseMap::new(&pose)?;
        resample_through(&map, floated, float, canvas, job)
    }
}

/// The resampling tail Puppet Warp and Perspective Warp share: the
/// premultiplied `floated` samples (standing at the canvas rect `float`)
/// through `map` onto the landing, the map's box ∩ canvas, with
/// `puppet_render::render`'s Catmull-Rom taps accumulated in binary64.
/// `Ok(None)` when the landing misses the canvas.
pub(crate) fn resample_through(
    map: &PoseMap,
    floated: &[[f32; 4]],
    float: CanvasRect,
    canvas: (u32, u32),
    job: JobContext<'_>,
) -> Result<Option<Landed>, String> {
    let Some(dest) = intersect(map.dest_rect(), (0, 0, canvas.0, canvas.1)) else {
        return Ok(None);
    };
    let mut out = reserved_vec(area(dest) as usize)?;
    out.resize(area(dest) as usize, [0.0f32; 4]);
    render(
        map,
        float,
        dest,
        |i| floated[i],
        |acc: &mut [f64; 4], p: [f32; 4], w| {
            for k in 0..4 {
                acc[k] += w * f64::from(p[k]);
            }
        },
        |acc| acc.map(|v| v as f32),
        |i, p| out[i] = p,
        job,
    )?;
    Ok(Some((dest, out)))
}
