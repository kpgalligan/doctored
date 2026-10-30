//! Perspective Warp's planes: the request's quads, their joins, the
//! straighten entries and each plane's homography. Free functions and value
//! types; `perspective_mesh` meshes a resolved [`Layout`], `perspective_warp`
//! extends it over the layer, and the adapters (`doc_perspective_warp`,
//! `precision_perspective_warp`, `selected_pixels_perspective`,
//! `ffi_perspective_warp`) parse through [`PerspectiveSpec::parse`].
//!
//! A QUAD is four corners `from` (where the plane stands in the layer) and
//! four `to` (where it goes), in canvas pixels, any consistent winding. Edge
//! `k` runs from corner `k` to corner `k + 1 mod 4`. With `e_k` that edge's
//! vector, every turn `z_k = e_k × e_{k+1}` must have one sign and
//! `|z_k| > 1e-6·|e_k||e_{k+1}|` (a convex quad with no three corners in a
//! line; the scale-free bound refuses a turn under about 1e-6 radians), else
//! the rest quad is degenerate and the warped one folds. A warped quad must
//! also turn the same way as its rest quad.
//!
//! JOINS. Scanning quads, then corners, a corner whose `from` equals an
//! earlier vertex's is that vertex (compared exactly, as the UI's snap writes
//! the same numbers), and its `to` must equal that vertex's too. A pair of
//! vertices that is a side of two or more quads is a SHARED EDGE: the planes
//! meet there, and `perspective_mesh` keeps them continuous along it.
//!
//! STRAIGHTEN. Each entry names a quad's edge and a direction; the first
//! entry per vertex pair that resolves is kept (a dropped `near_*` entry
//! leaves the pair to a later one, which is how the options bar's Straighten
//! Both asks for either axis). With θ the warped edge's angle folded to
//! [0°, 180°), `auto` resolves to horizontal within [`STRAIGHTEN_DEGREES`] of
//! 0° or 180°, to vertical within it of 90°, and otherwise to θ ("as drawn",
//! which moves nothing); `near_horizontal`/`near_vertical` resolve only to
//! their own axis and are otherwise dropped; `horizontal`, `vertical` and a
//! number of degrees are taken as given. Horizontal edges are grouped by
//! shared vertices (union-find) and each group gets one y: the anchor's (the
//! pin the UI is dragging) when it is in the group, else the group's mean in
//! vertex order; vertical groups likewise in x. Then each angle entry, in
//! list order, projects its endpoint(s) onto the line of its direction
//! through the anchor endpoint, or through the edge's midpoint.
//!
//! THE MAPS. A plane's forward map is `to ∘ from⁻¹` and its inverse
//! `from ∘ to⁻¹`, both composed from `doc_perspective::solve_rect_to_quad`
//! (the one homography solve) over the unit square, each scaled so its
//! homogeneous w is positive at its quad's centroid.
use serde_json::{json, Value};

use crate::doc_perspective::{solve_rect_to_quad, Homography};

/// The most quads a request carries.
pub(crate) const MAX_QUADS: usize = 64;
/// The most straighten entries a request carries.
pub(crate) const MAX_STRAIGHTEN: usize = 256;
/// `auto`'s and `near_*`'s reach from an axis, in degrees.
pub(crate) const STRAIGHTEN_DEGREES: f64 = 30.0;
/// The largest coordinate magnitude a corner may name, in canvas pixels.
const MAX_COORDINATE: f64 = 1_000_000.0;
/// The relative turn floor; see the module doc.
const TURN_EPSILON: f64 = 1e-6;
/// The fields text's byte cap, the strict request bound.
pub(crate) const FIELDS_LIMIT: usize = 64 * 1024;

/// One quad of the request.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaneQuad {
    pub(crate) from: [f64; 8],
    pub(crate) to: [f64; 8],
}

/// A straighten entry's direction as the request names it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wanted {
    Auto,
    NearHorizontal,
    NearVertical,
    Horizontal,
    Vertical,
    Degrees(f64),
}

/// One straighten entry of the request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Straighten {
    pub(crate) quad: usize,
    pub(crate) edge: usize,
    pub(crate) wanted: Wanted,
}

/// A resolved straighten direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Direction {
    Horizontal,
    Vertical,
    /// A line at this many degrees (clockwise on the y-down canvas).
    Angle(f64),
    /// `auto`'s "as drawn": the edge's own angle, so nothing moves.
    AsDrawn(f64),
}

impl Direction {
    fn describe(self) -> Value {
        match self {
            Direction::Horizontal => json!("horizontal"),
            Direction::Vertical => json!("vertical"),
            Direction::Angle(d) | Direction::AsDrawn(d) => json!(d),
        }
    }
}

/// Everything a request says besides its target.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PerspectiveSpec {
    pub(crate) quads: Vec<PlaneQuad>,
    pub(crate) straighten: Vec<Straighten>,
    pub(crate) anchor: Option<[f64; 2]>,
}

/// A resolved layout: shared vertices, quads over them, the shared edges,
/// the kept straighten entries and each quad's maps.
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    /// `[from, to]` per vertex, `to` after straightening.
    pub(crate) vertices: Vec<[[f64; 2]; 2]>,
    pub(crate) quads: Vec<[u32; 4]>,
    pub(crate) shared_edges: Vec<[u32; 2]>,
    pub(crate) straighten: Vec<(usize, usize, Direction)>,
    pub(crate) forward: Vec<[f64; 9]>,
    pub(crate) inverse: Vec<[f64; 9]>,
}

fn degenerate(i: usize) -> String {
    format!(
        "Perspective Warp quad {i} is degenerate: its corners must form a convex quad with no three in a line."
    )
}

fn folds(i: usize) -> String {
    format!(
        "Perspective Warp quad {i} folds: its warped corners must form a convex quad with no three in a line, turning the same way."
    )
}

fn straighten_refusal(s: usize) -> String {
    format!(
        "Perspective Warp straighten {s} must name a quad, an edge from 0 to 3 and a direction."
    )
}

const ANCHOR_REFUSAL: &str = "Perspective Warp anchor must be a quad corner.";

fn corners(value: Option<&Value>) -> Option<[f64; 8]> {
    let list = value?.as_array()?;
    if list.len() != 8 {
        return None;
    }
    let mut out = [0.0; 8];
    for (slot, v) in out.iter_mut().zip(list) {
        let f = v.as_f64()?;
        if !f.is_finite() || f.abs() > MAX_COORDINATE {
            return None;
        }
        *slot = f;
    }
    Some(out)
}

/// A whole number from 0 to `limit` inclusive (an integral float accepted).
fn index(value: Option<&Value>, limit: usize) -> Option<usize> {
    let v = value?;
    let n = match v.as_u64() {
        Some(n) => n,
        None => {
            let f = v.as_f64()?;
            if !(f >= 0.0 && f.fract() == 0.0 && f <= limit as f64) {
                return None;
            }
            f as u64
        }
    };
    usize::try_from(n).ok().filter(|&n| n <= limit)
}

fn wanted(value: Option<&Value>) -> Option<Wanted> {
    match value {
        None | Some(Value::Null) => Some(Wanted::Auto),
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(Wanted::Auto),
            "near_horizontal" => Some(Wanted::NearHorizontal),
            "near_vertical" => Some(Wanted::NearVertical),
            "horizontal" => Some(Wanted::Horizontal),
            "vertical" => Some(Wanted::Vertical),
            _ => None,
        },
        Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Wanted::Degrees),
    }
}

impl PerspectiveSpec {
    /// The strict parse of the request's `quads`, `straighten` and (layout
    /// and preview only) `anchor`. Refuses by name.
    pub(crate) fn parse(
        quads: Option<&Value>,
        straighten: Option<&Value>,
        anchor: Option<&Value>,
    ) -> Result<Self, String> {
        let list = match quads {
            Some(Value::Array(list)) => list.as_slice(),
            _ => &[][..],
        };
        if list.is_empty() {
            return Err("Perspective Warp needs at least one quad.".into());
        }
        if list.len() > MAX_QUADS {
            return Err(format!("Perspective Warp takes at most {MAX_QUADS} quads."));
        }
        let quads = list
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let refusal = || {
                    format!(
                        "Perspective Warp quad {i} needs from and to as eight finite numbers within 1000000."
                    )
                };
                let object = value.as_object().ok_or_else(refusal)?;
                if object.keys().any(|k| !matches!(k.as_str(), "from" | "to")) {
                    return Err(refusal());
                }
                Ok(PlaneQuad {
                    from: corners(object.get("from")).ok_or_else(refusal)?,
                    to: corners(object.get("to")).ok_or_else(refusal)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let entries = match straighten {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(list)) => list.as_slice(),
            Some(_) => return Err(straighten_refusal(0)),
        };
        if entries.len() > MAX_STRAIGHTEN {
            return Err(format!(
                "Perspective Warp takes at most {MAX_STRAIGHTEN} straighten entries."
            ));
        }
        let straighten = entries
            .iter()
            .enumerate()
            .map(|(s, value)| {
                let object = value.as_object().ok_or_else(|| straighten_refusal(s))?;
                if object
                    .keys()
                    .any(|k| !matches!(k.as_str(), "quad" | "edge" | "direction"))
                {
                    return Err(straighten_refusal(s));
                }
                let quad = index(object.get("quad"), quads.len() - 1);
                let edge = index(object.get("edge"), 3);
                let wanted = wanted(object.get("direction"));
                match (quad, edge, wanted) {
                    (Some(quad), Some(edge), Some(wanted)) => Ok(Straighten { quad, edge, wanted }),
                    _ => Err(straighten_refusal(s)),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        let anchor = match anchor {
            None | Some(Value::Null) => None,
            Some(v) => {
                let list = v
                    .as_array()
                    .filter(|l| l.len() == 2)
                    .ok_or(ANCHOR_REFUSAL)?;
                let x = list[0].as_f64().filter(|f| f.is_finite());
                let y = list[1].as_f64().filter(|f| f.is_finite());
                match (x, y) {
                    (Some(x), Some(y)) => Some([x, y]),
                    _ => return Err(ANCHOR_REFUSAL.into()),
                }
            }
        };
        Ok(Self {
            quads,
            straighten,
            anchor,
        })
    }

    /// The layout and preview fields `{"quads","straighten"?,"anchor"?}`,
    /// strict, at most 64 KiB.
    pub(crate) fn from_fields(text: &str) -> Result<Self, String> {
        if text.len() > FIELDS_LIMIT {
            return Err("perspective warp fields exceed 64 KiB".into());
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid perspective warp fields: {e}"))?;
        let object = value
            .as_object()
            .ok_or("invalid perspective warp fields: they must be a JSON object")?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "quads" | "straighten" | "anchor"))
        {
            return Err(format!(
                "invalid perspective warp fields: unknown field `{key}`"
            ));
        }
        Self::parse(
            object.get("quads"),
            object.get("straighten"),
            object.get("anchor"),
        )
    }

    /// Validate, join, straighten and solve; see the module doc.
    pub(crate) fn resolve(&self) -> Result<Layout, String> {
        let mut signs = Vec::with_capacity(self.quads.len());
        for (i, quad) in self.quads.iter().enumerate() {
            signs.push(turning(&quad.from).ok_or_else(|| degenerate(i))?);
        }
        // Joins.
        let mut vertices: Vec<[[f64; 2]; 2]> = Vec::new();
        let mut owner: Vec<usize> = Vec::new();
        let mut quads: Vec<[u32; 4]> = Vec::with_capacity(self.quads.len());
        for (i, quad) in self.quads.iter().enumerate() {
            let mut ids = [0u32; 4];
            for (k, id) in ids.iter_mut().enumerate() {
                let from = [quad.from[2 * k], quad.from[2 * k + 1]];
                let to = [quad.to[2 * k], quad.to[2 * k + 1]];
                match vertices.iter().position(|v| v[0] == from) {
                    Some(j) => {
                        if vertices[j][1] != to {
                            return Err(format!(
                                "Perspective Warp quads {} and {i} share a corner but warp it to different points.",
                                owner[j]
                            ));
                        }
                        *id = j as u32;
                    }
                    None => {
                        *id = vertices.len() as u32;
                        vertices.push([from, to]);
                        owner.push(i);
                    }
                }
            }
            quads.push(ids);
        }
        // Shared edges, in order of the side that makes them shared.
        let mut sides: Vec<([u32; 2], usize)> = Vec::new();
        let mut shared_edges = Vec::new();
        for ids in &quads {
            for k in 0..4 {
                let (a, b) = (ids[k], ids[(k + 1) % 4]);
                let key = [a.min(b), a.max(b)];
                match sides.iter_mut().find(|(s, _)| *s == key) {
                    Some((_, count)) => {
                        *count += 1;
                        if *count == 2 {
                            shared_edges.push([a, b]);
                        }
                    }
                    None => sides.push((key, 1)),
                }
            }
        }
        let anchor = match self.anchor {
            None => None,
            Some(p) => Some(
                vertices
                    .iter()
                    .position(|v| v[1] == p)
                    .ok_or(ANCHOR_REFUSAL)?,
            ),
        };
        // Straighten: resolve, then apply.
        let mut seen: Vec<[u32; 2]> = Vec::new();
        let mut straighten = Vec::new();
        for entry in &self.straighten {
            let ids = quads[entry.quad];
            let (a, b) = (ids[entry.edge], ids[(entry.edge + 1) % 4]);
            let key = [a.min(b), a.max(b)];
            if seen.contains(&key) {
                continue;
            }
            let (p, q) = (vertices[a as usize][1], vertices[b as usize][1]);
            let theta = (q[1] - p[1])
                .atan2(q[0] - p[0])
                .to_degrees()
                .rem_euclid(180.0);
            let near_h = theta <= STRAIGHTEN_DEGREES || theta >= 180.0 - STRAIGHTEN_DEGREES;
            let near_v = (theta - 90.0).abs() <= STRAIGHTEN_DEGREES;
            let direction = match entry.wanted {
                Wanted::Auto if near_h => Some(Direction::Horizontal),
                Wanted::Auto if near_v => Some(Direction::Vertical),
                Wanted::Auto => Some(Direction::AsDrawn(theta)),
                Wanted::NearHorizontal => near_h.then_some(Direction::Horizontal),
                Wanted::NearVertical => near_v.then_some(Direction::Vertical),
                Wanted::Horizontal => Some(Direction::Horizontal),
                Wanted::Vertical => Some(Direction::Vertical),
                Wanted::Degrees(d) => Some(Direction::Angle(d)),
            };
            if let Some(direction) = direction {
                seen.push(key);
                straighten.push((entry.quad, entry.edge, direction));
            }
        }
        let edge = |quad: usize, k: usize| -> (usize, usize) {
            let ids = quads[quad];
            (ids[k] as usize, ids[(k + 1) % 4] as usize)
        };
        for (axis, want) in [(1, Direction::Horizontal), (0, Direction::Vertical)] {
            let mut parent: Vec<usize> = (0..vertices.len()).collect();
            let mut member = vec![false; vertices.len()];
            for &(quad, k, direction) in &straighten {
                if direction != want {
                    continue;
                }
                let (a, b) = edge(quad, k);
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[rb] = ra;
                member[a] = true;
                member[b] = true;
            }
            let roots: Vec<usize> = (0..vertices.len()).map(|v| root(&mut parent, v)).collect();
            let mut done = vec![false; vertices.len()];
            for v in 0..vertices.len() {
                let r = roots[v];
                if !member[v] || done[r] {
                    continue;
                }
                done[r] = true;
                let group: Vec<usize> = (0..vertices.len())
                    .filter(|&u| member[u] && roots[u] == r)
                    .collect();
                let value = match anchor.filter(|a| group.contains(a)) {
                    Some(a) => vertices[a][1][axis],
                    None => {
                        group.iter().map(|&u| vertices[u][1][axis]).sum::<f64>()
                            / group.len() as f64
                    }
                };
                for &u in &group {
                    vertices[u][1][axis] = value;
                }
            }
        }
        for &(quad, k, direction) in &straighten {
            let Direction::Angle(degrees) = direction else {
                continue;
            };
            let (sin, cos) = degrees.to_radians().sin_cos();
            let (a, b) = edge(quad, k);
            let project = |o: [f64; 2], p: [f64; 2]| {
                let t = (p[0] - o[0]) * cos + (p[1] - o[1]) * sin;
                [o[0] + t * cos, o[1] + t * sin]
            };
            let (pa, pb) = (vertices[a][1], vertices[b][1]);
            if anchor == Some(a) {
                vertices[b][1] = project(pa, pb);
            } else if anchor == Some(b) {
                vertices[a][1] = project(pb, pa);
            } else {
                let m = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                vertices[a][1] = project(m, pa);
                vertices[b][1] = project(m, pb);
            }
        }
        // The warped quads and the maps.
        let mut forward = Vec::with_capacity(quads.len());
        let mut inverse = Vec::with_capacity(quads.len());
        for (i, ids) in quads.iter().enumerate() {
            let from = flat(ids.map(|v| vertices[v as usize][0]));
            let to = flat(ids.map(|v| vertices[v as usize][1]));
            if turning(&to) != Some(signs[i]) {
                return Err(folds(i));
            }
            solve_rect_to_quad((0.0, 0.0, 1.0, 1.0), &from).ok_or_else(|| degenerate(i))?;
            let (f, b) = plane_maps(&from, &to).ok_or_else(|| folds(i))?;
            forward.push(f);
            inverse.push(b);
        }
        Ok(Layout {
            vertices,
            quads,
            shared_edges,
            straighten,
            forward,
            inverse,
        })
    }
}

/// Union-find root with path halving.
fn root(parent: &mut [usize], mut v: usize) -> usize {
    while parent[v] != v {
        parent[v] = parent[parent[v]];
        v = parent[v];
    }
    v
}

fn flat(points: [[f64; 2]; 4]) -> [f64; 8] {
    let mut out = [0.0; 8];
    for (k, p) in points.iter().enumerate() {
        out[2 * k] = p[0];
        out[2 * k + 1] = p[1];
    }
    out
}

/// The sign of a convex quad's turns, or `None`; see the module doc.
pub(crate) fn turning(c: &[f64; 8]) -> Option<f64> {
    let p = |k: usize| [c[2 * (k % 4)], c[2 * (k % 4) + 1]];
    let mut sign = 0.0;
    for k in 0..4 {
        let (a, b, d) = (p(k), p(k + 1), p(k + 2));
        let e0 = [b[0] - a[0], b[1] - a[1]];
        let e1 = [d[0] - b[0], d[1] - b[1]];
        let z = e0[0] * e1[1] - e0[1] * e1[0];
        let bound = TURN_EPSILON * e0[0].hypot(e0[1]) * e1[0].hypot(e1[1]);
        if z.abs() <= bound || z.is_nan() {
            return None;
        }
        if sign == 0.0 {
            sign = z.signum();
        } else if z.signum() != sign {
            return None;
        }
    }
    Some(sign)
}

/// `a · b` for row-major 3×3 matrices.
fn multiply(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

/// The homogeneous w of `h` at `p`.
pub(crate) fn w_at(h: &[f64; 9], p: [f64; 2]) -> f64 {
    h[6] * p[0] + h[7] * p[1] + h[8]
}

/// `h` applied to `p`.
pub(crate) fn apply(h: &[f64; 9], p: [f64; 2]) -> [f64; 2] {
    let w = w_at(h, p);
    [
        (h[0] * p[0] + h[1] * p[1] + h[2]) / w,
        (h[3] * p[0] + h[4] * p[1] + h[5]) / w,
    ]
}

/// `h` scaled so its w is positive at the centroid of `quad`; `None` when it
/// is zero there or anything is not finite.
fn normalised(mut h: [f64; 9], quad: &[f64; 8]) -> Option<[f64; 9]> {
    let c = [
        (quad[0] + quad[2] + quad[4] + quad[6]) / 4.0,
        (quad[1] + quad[3] + quad[5] + quad[7]) / 4.0,
    ];
    let w = w_at(&h, c);
    if !w.is_finite() || w == 0.0 || !h.iter().all(|v| v.is_finite()) {
        return None;
    }
    if w < 0.0 {
        h.iter_mut().for_each(|v| *v = -*v);
    }
    Some(h)
}

/// The quad→quad maps `(forward, inverse)` of a plane; see the module doc.
pub(crate) fn plane_maps(from: &[f64; 8], to: &[f64; 8]) -> Option<([f64; 9], [f64; 9])> {
    let unit = (0.0, 0.0, 1.0, 1.0);
    let f = solve_rect_to_quad(unit, from)?;
    let t = solve_rect_to_quad(unit, to)?;
    let f_inverse = f.visible_inverse()?;
    let t_inverse = t.visible_inverse()?;
    let forward = normalised(multiply(&t.0, &f_inverse), from)?;
    let inverse = normalised(multiply(&f.0, &t_inverse), to)?;
    // A map whose determinant vanishes collapses the plane.
    if Homography(forward).determinant() == 0.0 || Homography(inverse).determinant() == 0.0 {
        return None;
    }
    Some((forward, inverse))
}

impl Layout {
    /// Every vertex goes where it stands.
    pub(crate) fn is_identity(&self) -> bool {
        self.vertices.iter().all(|v| v[0] == v[1])
    }

    /// Quad `q`'s rest corners.
    pub(crate) fn rest(&self, q: usize) -> [[f64; 2]; 4] {
        self.quads[q].map(|v| self.vertices[v as usize][0])
    }

    /// Quad `q`'s warped corners.
    pub(crate) fn warped(&self, q: usize) -> [[f64; 2]; 4] {
        self.quads[q].map(|v| self.vertices[v as usize][1])
    }

    /// `{"vertices":[{"from","to"}],"quads","shared_edges",
    /// "straighten":[{"quad","edge","direction"}],"identity"}`.
    pub(crate) fn describe(&self) -> Value {
        let vertices: Vec<Value> = self
            .vertices
            .iter()
            .map(|v| json!({"from": v[0], "to": v[1]}))
            .collect();
        let straighten: Vec<Value> = self
            .straighten
            .iter()
            .map(|&(quad, edge, direction)| {
                json!({"quad": quad, "edge": edge, "direction": direction.describe()})
            })
            .collect();
        json!({
            "vertices": vertices,
            "quads": self.quads,
            "shared_edges": self.shared_edges,
            "straighten": straighten,
            "identity": self.is_identity(),
        })
    }
}
