//! Perspective Warp's extension and map: how the rest of the layer
//! follows the planes. Free functions over a resolved
//! `perspective_plane::Layout` and its `perspective_mesh::PlaneMesh`; the
//! adapters (`doc_perspective_warp`, `precision_perspective_warp`,
//! `selected_pixels_perspective`, `ffi_perspective_warp`) call [`prepare`]
//! and [`map`].
//!
//! THE EXTENSION. The subject is the layer rect; the grid covers it and the
//! rest quads' outward bounds (`puppet_mesh::grid`, Normal density). Its
//! vertices are pinned, first rule wins: (a) a vertex inside (closed) a rest
//! quad goes where the rendered plane puts it (`PlaneMesh::forward_at`, the
//! highest-index such quad); (b) otherwise a vertex of a grid triangle whose
//! rest triangle meets a rest quad (that quad's RING, highest index) goes to
//! `H_q(v)` when `w_q(v) ≥ ½·min` over the quad's corners of `w_q`, and else
//! moves with the quad's nearest corner, `v + (to − from)`. The guard keeps
//! the homography away from its vanishing line, where it runs to infinity
//! and flips; half the corners' least w is well inside the side the quad
//! lives on. The rest of the grid follows by `puppet_arap::solve` in Distort
//! mode (shear resisted, scale free), Puppet Warp's one ARAP solve.
//!
//! THE MAP. Grid triangles come first in index order (tag `u32::MAX`), then
//! the plane mesh on top (tag = quad). A grid triangle whose three vertices
//! were all pinned by `H_q` of one projective quad `q` takes its source
//! through `q`'s inverse homography, so the map continues the plane exactly
//! across its border; every other grid triangle is piecewise affine at rest.
//! The destination is the outward bounds of every map triangle's warped
//! corners. Resampling is `puppet_render::render` (Catmull-Rom, the Free
//! Transform default and Puppet Warp's); there is no filter option.
//!
//! ADMISSION (the plan's terms): [`GRID_BYTES`] for the grid and its pins,
//! `puppet_arap::SOLVER_BYTES`, `puppet_render::MAP_BYTES`, [`TRIANGLE_BYTES`]
//! per plane triangle and 2 MiB of bookkeeping ([`scratch`]), plus each
//! depth's source copy, output and `puppet_render::MARGIN_BYTES_PER_PIXEL`
//! per destination pixel (`doc_puppet::mesh_warp_plan`).
//!
//! Record (the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, a fresh
//! instance per case, `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint`
//! − pre around the one `perspective_warp` call, 3 runs + a baseline twin;
//! `scripts/perspective-warp-admission/measure.py`; Apple M2 Max, 96 GiB,
//! 2026-10-13): a 6000×4000 one-layer document of two-colour Clouds. P is the
//! host's admitted plan (the core plan's `scratch_bytes` plus the host's 24n
//! projection, `MeshWarpWork.prepareLayer`), declared before the runs.
//! - 8-bit, one keystone quad over most of the layer (warped to 6319×4039):
//!   P 941,629,224; E 391,479,536 / 391,397,592 / 391,512,304 (2.405×).
//! - 16-bit, the same: P 1,139,718,988; E 374,374,616 / 370,622,680 /
//!   373,588,208 (3.044×).
//! - 32-bit float, the same: P 1,944,283,172; E 793,330,240 / 793,264,704 /
//!   793,264,704 (2.451×).
//! - The selection's float at 8 bits (a 4000×2400 selection, its quad
//!   keystoned): P 703,244,000; E 431,145,296 / 431,128,888 / 431,096,120
//!   (1.631×).
//! - 8-bit, two quads joined along x = 3000 and warped so their homographies
//!   slide there (an affine group at n = 64, 16,384 plane triangles; warped
//!   to 6085×4088): P 940,647,328; E 388,579,568 / 388,727,024 / 388,579,568
//!   (2.420×).
//!
//! The baseline twins read E ≤ 49,152. Every case clears ⌈1.25 × max E⌉ at
//! the margin Puppet Warp set, so it stays 4; a miss raises
//! `MARGIN_BYTES_PER_PIXEL` only.
use serde_json::Value;

use crate::job_control::JobContext;
use crate::perspective_mesh::{build, plane_triangles, PlaneMesh};
use crate::perspective_plane::{apply, w_at, Layout, PerspectiveSpec};
use crate::precision_image::admit;
use crate::puppet_arap::{solve, Mode, Pin, PuppetPose, SOLVER_BYTES};
use crate::puppet_mesh::{grid, outward_bounds, Density, PuppetMesh, Rect};
use crate::puppet_render::{MapTriangle, PoseMap, TriangleSource, MAP_BYTES};
use crate::selected_pixels_walk::union;

/// The grid, its pin targets and classification (`O(vertices + triangles)`
/// at the 3000-vertex cap), at most.
pub(crate) const GRID_BYTES: u64 = 1 << 20;
/// One plane triangle's record, its map triangle and its bucket share.
pub(crate) const TRIANGLE_BYTES: u64 = 256;
/// Fixed bookkeeping on top of the terms above.
const BOOKKEEPING: u64 = 2 * 1024 * 1024;
pub(crate) const PIXEL_LIMIT_REFUSAL: &str =
    "Perspective Warp would make the layer larger than the pixel limit.";
/// The grid tag of a map triangle.
const EXTENSION: u32 = u32::MAX;

/// The warp's working memory besides each depth's source copy and output.
pub(crate) fn scratch(layout: &Layout) -> u64 {
    GRID_BYTES
        .saturating_add(SOLVER_BYTES)
        .saturating_add(MAP_BYTES)
        .saturating_add(TRIANGLE_BYTES.saturating_mul(plane_triangles(layout) as u64))
        .saturating_add(BOOKKEEPING)
}

/// The prepared warp both the plan and the edit start from. Made once per
/// call and moved, never stored, so its size does not matter.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Prepared {
    Identity,
    Ready {
        pose: PuppetPose,
        layout: Layout,
        plane: PlaneMesh,
        dest: Rect,
        vertices: usize,
    },
}

/// Is `p` inside (closed) the convex quad `c`?
fn quad_contains(c: &[[f64; 2]; 4], p: [f64; 2]) -> bool {
    let mut sign = 0.0f64;
    for k in 0..4 {
        let (a, b) = (c[k], c[(k + 1) % 4]);
        let z = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if z == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = z.signum();
        } else if z.signum() != sign {
            return false;
        }
    }
    true
}

/// Do the convex polygons `a` and `b` meet (touching counts)? Separating
/// axes over both polygons' edge normals.
fn meets(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    for poly in [a, b] {
        for k in 0..poly.len() {
            let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
            let normal = [q[1] - p[1], p[0] - q[0]];
            let range = |s: &[[f64; 2]]| {
                s.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                        let d = v[0] * normal[0] + v[1] * normal[1];
                        (lo.min(d), hi.max(d))
                    })
            };
            let (alo, ahi) = range(a);
            let (blo, bhi) = range(b);
            if alo.max(blo) > ahi.min(bhi) {
                return false;
            }
        }
    }
    true
}

/// The extension's pins and, per grid vertex, the projective quad whose
/// homography pinned it; see the module doc.
fn extension_pins(
    layout: &Layout,
    plane: &PlaneMesh,
    mesh: &PuppetMesh,
    job: JobContext<'_>,
) -> Result<(Vec<Pin>, Vec<Option<usize>>), String> {
    let rest = mesh.vertices();
    let count = layout.quads.len();
    let quads: Vec<[[f64; 2]; 4]> = (0..count).map(|q| layout.rest(q)).collect();
    let boxes: Vec<[f64; 4]> = quads
        .iter()
        .map(|c| {
            c.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                },
            )
        })
        .collect();
    let in_box =
        |b: &[f64; 4], p: [f64; 2]| p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3];
    let mut inside: Vec<Option<usize>> = vec![None; rest.len()];
    for (q, c) in quads.iter().enumerate() {
        job.check()?;
        for (v, &p) in rest.iter().enumerate() {
            if in_box(&boxes[q], p) && quad_contains(c, p) {
                inside[v] = Some(q);
            }
        }
    }
    let mut ring: Vec<Option<usize>> = vec![None; rest.len()];
    for (t, tri) in mesh.triangles().iter().enumerate() {
        job.every(t)?;
        let corners = tri.map(|v| rest[v as usize]);
        let tbox = corners.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            },
        );
        for (q, c) in quads.iter().enumerate() {
            let b = &boxes[q];
            let overlap = tbox[0] <= b[2] && b[0] <= tbox[2] && tbox[1] <= b[3] && b[1] <= tbox[3];
            if overlap && meets(&corners, c) {
                for &v in tri {
                    let slot = &mut ring[v as usize];
                    *slot = Some(slot.map_or(q, |r| r.max(q)));
                }
            }
        }
    }
    let mut pins = Vec::new();
    let mut by_plane: Vec<Option<usize>> = vec![None; rest.len()];
    for (v, &p) in rest.iter().enumerate() {
        let target = if let Some(q) = inside[v] {
            if plane.projective[q] {
                by_plane[v] = Some(q);
            }
            plane
                .forward_at(layout, q, p)
                .unwrap_or_else(|| apply(&layout.forward[q], p))
        } else if let Some(q) = ring[v] {
            let h = &layout.forward[q];
            let least = quads[q]
                .iter()
                .map(|&c| w_at(h, c))
                .fold(f64::INFINITY, f64::min);
            if w_at(h, p) >= 0.5 * least {
                if plane.projective[q] {
                    by_plane[v] = Some(q);
                }
                apply(h, p)
            } else {
                let warped = layout.warped(q);
                let mut nearest = 0;
                let mut best = f64::INFINITY;
                for (k, c) in quads[q].iter().enumerate() {
                    let d = (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2);
                    if d < best {
                        best = d;
                        nearest = k;
                    }
                }
                let (from, to) = (quads[q][nearest], warped[nearest]);
                [p[0] + (to[0] - from[0]), p[1] + (to[1] - from[1])]
            }
        } else {
            continue;
        };
        pins.push(Pin {
            from: p,
            to: target,
            depth: 0,
            rotate: None,
        });
    }
    Ok((pins, by_plane))
}

/// Resolve, mesh, pin and solve `spec` over the layer standing at `subject`,
/// admitting the grid, the solver and the plane triangles first.
pub(crate) fn prepare(
    spec: &PerspectiveSpec,
    subject: Rect,
    limit: u64,
    other: u64,
    job: JobContext<'_>,
) -> Result<Prepared, String> {
    let layout = spec.resolve()?;
    if layout.is_identity() {
        return Ok(Prepared::Identity);
    }
    admit(
        0,
        GRID_BYTES
            .saturating_add(SOLVER_BYTES)
            .saturating_add(TRIANGLE_BYTES.saturating_mul(plane_triangles(&layout) as u64)),
        other,
        limit,
    )?;
    let plane = build(&layout)?;
    let corners: Vec<[f64; 2]> = layout.vertices.iter().map(|v| v[0]).collect();
    let rect = union(subject, outward_bounds(&corners));
    let mesh = grid(rect, Density::Normal)?;
    let (pins, _) = extension_pins(&layout, &plane, &mesh, job)?;
    let pose = solve(&mesh, &pins, Mode::Distort, job)?;
    let warped: Vec<[f64; 2]> = pose
        .positions()
        .iter()
        .chain(&plane.warped)
        .copied()
        .collect();
    let dest = outward_bounds(&warped);
    if u64::from(dest.2) * u64::from(dest.3) > crate::doc::MAX_PIXELS
        || i32::try_from(dest.0).is_err()
        || i32::try_from(dest.1).is_err()
        || !warped.iter().all(|p| p[0].is_finite() && p[1].is_finite())
    {
        return Err(PIXEL_LIMIT_REFUSAL.into());
    }
    let vertices = mesh.vertices().len() + plane.rest.len();
    Ok(Prepared::Ready {
        pose,
        layout,
        plane,
        dest,
        vertices,
    })
}

/// The inverse map of a prepared warp; see the module doc.
pub(crate) fn map(
    layout: &Layout,
    plane: &PlaneMesh,
    pose: &PuppetPose,
) -> Result<PoseMap, String> {
    let mesh = pose.mesh();
    let (_, by_plane) = extension_pins(layout, plane, mesh, JobContext::NONE)?;
    let rest = mesh.vertices();
    let positions = pose.positions();
    let mut triangles: Vec<MapTriangle> =
        Vec::with_capacity(mesh.triangles().len() + plane.triangles.len());
    for tri in mesh.triangles() {
        let q = by_plane[tri[0] as usize];
        let projective =
            q.is_some() && by_plane[tri[1] as usize] == q && by_plane[tri[2] as usize] == q;
        let source = match q {
            Some(q) if projective => TriangleSource::Projective(layout.inverse[q]),
            _ => TriangleSource::Rest(tri.map(|v| rest[v as usize])),
        };
        triangles.push(MapTriangle {
            corners: tri.map(|v| positions[v as usize]),
            source,
            tag: EXTENSION,
        });
    }
    triangles.extend(plane.map_triangles(layout));
    PoseMap::from_triangles(triangles, mesh.spacing())
}

/// One probed point: the plane index (`None` for the extension) and the
/// source point, or `None` where nothing maps.
pub type ProbedPoint = Option<(Option<usize>, [f64; 2])>;

/// Test seam: `fields` (`{"quads","straighten"?,"anchor"?}`) prepared over
/// the layer rect `subject`, and each canvas point of `points` through
/// [`map`]. An identity layout answers each point itself.
#[doc(hidden)]
pub fn probe(
    fields: &str,
    subject: (i64, i64, u32, u32),
    points: &[[f64; 2]],
) -> Result<Vec<ProbedPoint>, String> {
    let spec = PerspectiveSpec::from_fields(fields)?;
    match prepare(&spec, subject, u64::MAX, 0, JobContext::NONE)? {
        Prepared::Identity => Ok(points.iter().map(|&p| Some((None, p))).collect()),
        Prepared::Ready {
            pose,
            layout,
            plane,
            ..
        } => {
            let map = map(&layout, &plane, &pose)?;
            Ok(points
                .iter()
                .map(|&p| {
                    map.locate(p)
                        .map(|(tag, s)| ((tag != EXTENSION).then_some(tag as usize), s))
                })
                .collect())
        }
    }
}

/// The resolved layout of `fields` (`{"quads","straighten"?,"anchor"?}`),
/// described; the FFI's layout call.
pub(crate) fn describe_fields(fields: &str) -> Result<Value, String> {
    Ok(PerspectiveSpec::from_fields(fields)?.resolve()?.describe())
}
