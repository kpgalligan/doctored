//! Perspective Warp's plane mesh: the triangles a resolved
//! `perspective_plane::Layout` renders its planes through. Free functions
//! and one value type; `perspective_warp` lays them over its extension.
//!
//! GROUPS are the connected components of quads joined through shared
//! edges. A group is PROJECTIVE when, on every shared edge, the forward maps
//! of the quads holding it agree within `SEAM_EPSILON·max(1, rest edge
//! length)` px at the edge's ¼, ½ and ¾ points: a projective map of a line
//! is fixed by three points, and the joined corners already agree, so three
//! more interior points settle it. A projective quad renders as two
//! triangles, `[c0, c1, c2]` and `[c0, c2, c3]`, whose sources go through
//! the quad's own inverse homography: exact, and continuous across every
//! shared edge because the maps agree there.
//!
//! An AFFINE group (its homographies slide along a shared edge) renders each
//! quad as an `n × n` grid, `n = clamp(⌈L / SUB_TARGET_PX⌉, 4, 64)` with `L`
//! the longest warped side in the group, every quad of the group at the same
//! `n` so their shared sides hold the same points. With `c0 = (0,0)`,
//! `c1 = (1,0)`, `c2 = (1,1)`, `c3 = (0,1)`, a sub-vertex's rest point is the
//! bilinear blend of the rest corners, and its warped point is
//! `H_q(rest) + C_q(u, v)`, the Coons patch of the side corrections
//! `C_q = (1−v)·d(u,0) + u·d(1,v) + v·d(u,1) + (1−u)·d(0,v)`. On an unshared
//! side `d` is 0, so the side is the plane's own straight border; on a shared
//! side `d(r)` is the mean of the side's quads' `H_s(r)` less `H_q(r)`, so
//! every quad lands the side's points on the same mean. It is 0 at corners.
//! Each shared side's `n − 1` interior sub-vertices are made once, by its
//! lowest-index quad, and reused by every quad holding it, so the two sides
//! of a seam hold the very same vertex pairs and the map has no gap or
//! overlap there. Cells split on the `(i,j)–(i+1,j+1)` diagonal and take
//! their sources at rest (piecewise affine). A sub-triangle that turns the
//! other way once warped refuses with its quad's fold sentence.
use crate::perspective_plane::{apply, Layout};
use crate::puppet_render::{MapTriangle, TriangleSource};

/// The seam agreement bound, relative to the rest edge length; see the
/// module doc. A millionth of a pixel per pixel of edge is far below what a
/// resample can show, and far above binary64 noise in the composed maps.
pub(crate) const SEAM_EPSILON: f64 = 1e-6;
/// The affine sub-grid's target cell side in warped pixels.
pub(crate) const SUB_TARGET_PX: f64 = 16.0;
pub(crate) const MIN_SUBDIVISIONS: u32 = 4;
pub(crate) const MAX_SUBDIVISIONS: u32 = 64;

/// The rendered plane mesh.
#[derive(Clone, Debug)]
pub(crate) struct PlaneMesh {
    pub(crate) rest: Vec<[f64; 2]>,
    pub(crate) warped: Vec<[f64; 2]>,
    /// Each triangle's vertices and its quad, quads ascending.
    pub(crate) triangles: Vec<([u32; 3], u32)>,
    pub(crate) projective: Vec<bool>,
    /// Each quad's triangle range.
    ranges: Vec<(usize, usize)>,
}

/// Union-find root with path halving.
fn root(parent: &mut [usize], mut v: usize) -> usize {
    while parent[v] != v {
        parent[v] = parent[parent[v]];
        v = parent[v];
    }
    v
}

/// The quads holding each shared edge as a side.
fn edge_quads(layout: &Layout) -> Vec<Vec<usize>> {
    layout
        .shared_edges
        .iter()
        .map(|&[a, b]| {
            (0..layout.quads.len())
                .filter(|&q| side_of(layout, q, a, b).is_some())
                .collect()
        })
        .collect()
}

/// The side `k` of quad `q` whose ends are `a` and `b` (either way).
fn side_of(layout: &Layout, q: usize, a: u32, b: u32) -> Option<usize> {
    let ids = layout.quads[q];
    (0..4).find(|&k| {
        let (s, t) = (ids[k], ids[(k + 1) % 4]);
        (s == a && t == b) || (s == b && t == a)
    })
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Per quad: 1 for a projective group, else its group's `n`.
pub(crate) fn subdivisions(layout: &Layout) -> Vec<u32> {
    let count = layout.quads.len();
    let holders = edge_quads(layout);
    let mut parent: Vec<usize> = (0..count).collect();
    for quads in &holders {
        for pair in quads.windows(2) {
            let (a, b) = (root(&mut parent, pair[0]), root(&mut parent, pair[1]));
            parent[b] = a;
        }
    }
    let group: Vec<usize> = (0..count).map(|q| root(&mut parent, q)).collect();
    let mut affine = vec![false; count];
    for (edge, quads) in layout.shared_edges.iter().zip(&holders) {
        let a = layout.vertices[edge[0] as usize][0];
        let b = layout.vertices[edge[1] as usize][0];
        let tolerance = SEAM_EPSILON * distance(a, b).max(1.0);
        let agrees = [0.25, 0.5, 0.75].iter().all(|&t| {
            let r = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
            let first = apply(&layout.forward[quads[0]], r);
            quads[1..]
                .iter()
                .all(|&s| distance(apply(&layout.forward[s], r), first) <= tolerance)
        });
        if !agrees {
            affine[group[quads[0]]] = true;
        }
    }
    let mut longest = vec![0.0f64; count];
    for (q, &g) in group.iter().enumerate() {
        let w = layout.warped(q);
        for k in 0..4 {
            longest[g] = longest[g].max(distance(w[k], w[(k + 1) % 4]));
        }
    }
    (0..count)
        .map(|q| {
            let g = group[q];
            if !affine[g] {
                return 1;
            }
            let n = (longest[g] / SUB_TARGET_PX).ceil();
            n.clamp(f64::from(MIN_SUBDIVISIONS), f64::from(MAX_SUBDIVISIONS)) as u32
        })
        .collect()
}

/// The plane mesh's triangle count.
pub(crate) fn plane_triangles(layout: &Layout) -> usize {
    subdivisions(layout)
        .iter()
        .map(|&n| 2 * n as usize * n as usize)
        .sum()
}

fn fold(q: usize) -> String {
    format!(
        "Perspective Warp quad {q} folds: its warped corners must form a convex quad with no three in a line, turning the same way."
    )
}

fn cross(t: [[f64; 2]; 3]) -> f64 {
    (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])
}

/// The bilinear blend of `c` at `(u, v)`.
fn blend(c: &[[f64; 2]; 4], u: f64, v: f64) -> [f64; 2] {
    let w = [(1.0 - u) * (1.0 - v), u * (1.0 - v), u * v, (1.0 - u) * v];
    [
        w[0] * c[0][0] + w[1] * c[1][0] + w[2] * c[2][0] + w[3] * c[3][0],
        w[0] * c[0][1] + w[1] * c[1][1] + w[2] * c[2][1] + w[3] * c[3][1],
    ]
}

/// Grid point `(i, j)` of side `k`'s `t`-th step from corner `k`.
fn side_point(k: usize, t: u32, n: u32) -> (u32, u32) {
    match k {
        0 => (t, 0),
        1 => (n, t),
        2 => (n - t, n),
        _ => (0, n - t),
    }
}

/// Build the plane mesh; see the module doc.
pub(crate) fn build(layout: &Layout) -> Result<PlaneMesh, String> {
    let subs = subdivisions(layout);
    let holders = edge_quads(layout);
    let mut rest: Vec<[f64; 2]> = layout.vertices.iter().map(|v| v[0]).collect();
    let mut warped: Vec<[f64; 2]> = layout.vertices.iter().map(|v| v[1]).collect();
    let mut triangles = Vec::new();
    let mut ranges = Vec::with_capacity(layout.quads.len());
    // A shared side's interior sub-vertices: (the corner they run from, ids).
    let mut made: Vec<(usize, u32, Vec<u32>)> = Vec::new();
    for (q, ids) in layout.quads.iter().enumerate() {
        let start = triangles.len();
        let n = subs[q];
        if n == 1 {
            let tag = q as u32;
            triangles.push(([ids[0], ids[1], ids[2]], tag));
            triangles.push(([ids[0], ids[2], ids[3]], tag));
            ranges.push((start, triangles.len()));
            continue;
        }
        let corners = layout.rest(q);
        let h = &layout.forward[q];
        // The side corrections d at each side step (0 at the corners).
        let shared: Vec<Option<usize>> = (0..4)
            .map(|k| {
                let (a, b) = (ids[k], ids[(k + 1) % 4]);
                layout
                    .shared_edges
                    .iter()
                    .position(|e| (e[0] == a && e[1] == b) || (e[0] == b && e[1] == a))
            })
            .collect();
        let nf = f64::from(n);
        let param = |(i, j): (u32, u32)| (f64::from(i) / nf, f64::from(j) / nf);
        let d: Vec<Vec<[f64; 2]>> = (0..4)
            .map(|k| {
                (0..=n)
                    .map(|t| {
                        let Some(e) = shared[k].filter(|_| t > 0 && t < n) else {
                            return [0.0, 0.0];
                        };
                        let (u, v) = param(side_point(k, t, n));
                        let r = blend(&corners, u, v);
                        let own = apply(h, r);
                        let quads = &holders[e];
                        let mut mean = [0.0, 0.0];
                        for &s in quads {
                            let p = apply(&layout.forward[s], r);
                            mean[0] += p[0];
                            mean[1] += p[1];
                        }
                        let m = quads.len() as f64;
                        [mean[0] / m - own[0], mean[1] / m - own[1]]
                    })
                    .collect()
            })
            .collect();
        let at = |k: usize, t: u32| d[k][t as usize];
        let make = |i: u32, j: u32, rest: &mut Vec<[f64; 2]>, warped: &mut Vec<[f64; 2]>| {
            let (u, v) = param((i, j));
            let r = blend(&corners, u, v);
            let base = apply(h, r);
            // d(u,0) is side 0 at step i; d(1,v) side 1 at step j; d(u,1)
            // side 2 at step n−i; d(0,v) side 3 at step n−j.
            let c = [0, 1].map(|x| {
                (1.0 - v) * at(0, i)[x]
                    + u * at(1, j)[x]
                    + v * at(2, n - i)[x]
                    + (1.0 - u) * at(3, n - j)[x]
            });
            rest.push(r);
            warped.push([base[0] + c[0], base[1] + c[1]]);
            (rest.len() - 1) as u32
        };
        let size = (n + 1) as usize;
        let mut grid = vec![u32::MAX; size * size];
        let slot = |i: u32, j: u32| j as usize * size + i as usize;
        for (k, (ci, cj)) in [(0, 0), (n, 0), (n, n), (0, n)].into_iter().enumerate() {
            grid[slot(ci, cj)] = ids[k];
        }
        for k in 0..4 {
            let from = ids[k];
            let existing = shared[k].and_then(|e| {
                made.iter()
                    .find(|(edge, ..)| *edge == e)
                    .map(|(_, start, list)| (*start, list.clone()))
            });
            match existing {
                Some((start, list)) => {
                    for t in 1..n {
                        let step = if start == from { t } else { n - t };
                        let (i, j) = side_point(k, t, n);
                        grid[slot(i, j)] = list[step as usize - 1];
                    }
                }
                None => {
                    let mut list = Vec::with_capacity(n as usize - 1);
                    for t in 1..n {
                        let (i, j) = side_point(k, t, n);
                        let id = make(i, j, &mut rest, &mut warped);
                        grid[slot(i, j)] = id;
                        list.push(id);
                    }
                    if let Some(e) = shared[k] {
                        made.push((e, from, list));
                    }
                }
            }
        }
        for j in 1..n {
            for i in 1..n {
                grid[slot(i, j)] = make(i, j, &mut rest, &mut warped);
            }
        }
        let tag = q as u32;
        for j in 0..n {
            for i in 0..n {
                let a = grid[slot(i, j)];
                let b = grid[slot(i + 1, j)];
                let c = grid[slot(i + 1, j + 1)];
                let e = grid[slot(i, j + 1)];
                for tri in [[a, b, c], [a, c, e]] {
                    let r = cross(tri.map(|v| rest[v as usize]));
                    let w = cross(tri.map(|v| warped[v as usize]));
                    if r * w <= 0.0 || (r * w).is_nan() {
                        return Err(fold(q));
                    }
                    triangles.push((tri, tag));
                }
            }
        }
        ranges.push((start, triangles.len()));
    }
    Ok(PlaneMesh {
        rest,
        warped,
        triangles,
        projective: subs.iter().map(|&n| n == 1).collect(),
        ranges,
    })
}

/// The barycentric coordinates of `p` in `t`, `None` when degenerate.
fn barycentric(t: [[f64; 2]; 3], p: [f64; 2]) -> Option<[f64; 3]> {
    let d = cross(t);
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let l1 = ((p[0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (p[1] - t[0][1])) / d;
    let l2 = ((t[1][0] - t[0][0]) * (p[1] - t[0][1]) - (p[0] - t[0][0]) * (t[1][1] - t[0][1])) / d;
    Some([1.0 - l1 - l2, l1, l2])
}

impl PlaneMesh {
    /// The rendered map at the rest point `p` inside quad `quad`: its
    /// homography for a projective quad, else the sub-triangle holding `p`
    /// (closed, 1e-9 barycentric slack) applied at rest.
    pub(crate) fn forward_at(&self, layout: &Layout, quad: usize, p: [f64; 2]) -> Option<[f64; 2]> {
        if self.projective[quad] {
            return Some(apply(&layout.forward[quad], p));
        }
        let (start, end) = self.ranges[quad];
        for (tri, _) in &self.triangles[start..end] {
            let Some(l) = barycentric(tri.map(|v| self.rest[v as usize]), p) else {
                continue;
            };
            if l.iter().all(|&x| x >= -1e-9) {
                let w = tri.map(|v| self.warped[v as usize]);
                return Some([
                    l[0] * w[0][0] + l[1] * w[1][0] + l[2] * w[2][0],
                    l[0] * w[0][1] + l[1] * w[1][1] + l[2] * w[2][1],
                ]);
            }
        }
        None
    }

    /// The map triangles, quads ascending (a later quad draws on top), each
    /// tagged with its quad.
    pub(crate) fn map_triangles(&self, layout: &Layout) -> Vec<MapTriangle> {
        self.triangles
            .iter()
            .map(|&(tri, quad)| MapTriangle {
                corners: tri.map(|v| self.warped[v as usize]),
                source: if self.projective[quad as usize] {
                    TriangleSource::Projective(layout.inverse[quad as usize])
                } else {
                    TriangleSource::Rest(tri.map(|v| self.rest[v as usize]))
                },
                tag: quad,
            })
            .collect()
    }
}
