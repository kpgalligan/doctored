//! Puppet Warp's pins and its as-rigid-as-possible solve. Free
//! functions and value types over a `puppet_mesh::PuppetMesh`; the adapters
//! (`doc_puppet`, `precision_puppet`, `selected_pixels_puppet`, `ffi_puppet`)
//! parse a request through [`PuppetSpec::parse`] and call [`solve`].
//!
//! THE PINS. A pin's `from` must lie in a rest triangle (else "pin i is
//! outside the mesh"); it snaps to that triangle's nearest vertex `v`
//! (`PuppetMesh::snap`). Its target is `to` itself when `from` IS `v`'s rest
//! position, else `v + (to − from)`, so a pin dragged by the UI from its
//! snapped vertex lands exactly where the pointer put it, and a request whose
//! `from` misses the vertex by a little still moves it by the drag. Two pins
//! on one vertex refuse. Pinned vertices are written, never solved, so each
//! lands on its target bit for bit.
//!
//! THE ENERGY. With `J_t` the 2×2 deformation gradient of rest triangle `t`
//! (the piecewise-affine map's Jacobian, linear in the deformed positions)
//! and `A_t` its rest area, the solve minimizes `E = Σ A_t‖J_t − T_t‖²_F`
//! by local–global alternation (Sorkine & Alexa's ARAP, on triangles as in
//! Igarashi's shape manipulation). The GLOBAL step is the linear
//! least-squares problem for fixed `T_t`: one symmetric positive definite
//! system (the cotangent Laplacian with the pinned and resting vertices
//! eliminated) shared by x and y, factored once per pinned set
//! (`puppet_cholesky`). A connected component of the mesh
//! with no pin stays at rest: its vertices are eliminated at their rest
//! positions, which is also what keeps the system definite. The LOCAL step
//! fits each `T_t` to `J_t`: the rotation `R(φ)` closest to `J_t`
//! (`φ = atan2(J₁₀ − J₀₁, J₀₀ + J₁₁)`) and the least-squares uniform scale
//! along it, `s = tr(R(φ)ᵀJ_t)/2` (for an unflipped triangle that is
//! `(σ₁ + σ₂)/2`, the mean singular value). Rigid takes `T = R`, Normal
//! `T = ((1 + s)/2)·R` (half way between rigid and similar), Distort
//! `T = s·R`, so Rigid resists every stretch, Distort only shear.
//!
//! ROTATION. A pin with a Fixed `rotate` (degrees, clockwise-positive on the
//! y-down canvas, which is the standard `R(φ)` there) forces `φ` on the
//! triangles at its vertex; where two Fixed pins share a triangle the lower
//! pin index wins. Auto leaves `φ` to the fit.
//!
//! CONVERGENCE. The first global step runs with every `T_t = I`; each later
//! one follows a local step, until no vertex moves by 1e-4 px or more, or 50
//! iterations. Every step is a deterministic function of (mesh, pins, mode),
//! so a preview and a commit of the same pins agree bit for bit.
use serde_json::{json, Value};

use crate::job_control::JobContext;
use crate::puppet_cholesky::factor;
use crate::puppet_mesh::{outward_bounds, whole, MeshOptions, PuppetMesh, MAX_MESH_VERTICES};

/// The most pins a request carries.
pub(crate) const MAX_PINS: usize = 256;
/// The solver's working memory at the vertex cap: the dense lower triangle
/// of an `n = MAX_MESH_VERTICES` envelope at 8 bytes, the worst any ordering
/// can store, plus 4 MiB for the triplets, gradients, positions and the RCM
/// bookkeeping (each `O(n + triangles)`).
pub(crate) const SOLVER_BYTES: u64 =
    8 * (MAX_MESH_VERTICES as u64 * (MAX_MESH_VERTICES as u64 + 1) / 2) + 4 * 1024 * 1024;
/// The largest coordinate magnitude a pin may name, in canvas pixels.
const MAX_COORDINATE: f64 = 1_000_000.0;
const MAX_DEPTH: i64 = 1000;
/// The stop rule: no vertex moved by this much (px) in the last iteration.
const TOLERANCE: f64 = 1e-4;
const MAX_ITERATIONS: usize = 50;

const MODE_REFUSAL: &str = "Puppet Warp mode must be rigid, normal or distort.";

/// The options bar's Mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Rigid,
    Normal,
    Distort,
}

impl Mode {
    /// `"rigid" | "normal" | "distort"`; absent or null is Normal.
    pub(crate) fn parse(value: Option<&Value>) -> Result<Self, String> {
        match value {
            None | Some(Value::Null) => Ok(Mode::Normal),
            Some(Value::String(s)) => match s.as_str() {
                "rigid" => Ok(Mode::Rigid),
                "normal" => Ok(Mode::Normal),
                "distort" => Ok(Mode::Distort),
                _ => Err(MODE_REFUSAL.into()),
            },
            Some(_) => Err(MODE_REFUSAL.into()),
        }
    }
}

/// One validated pin, canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pin {
    pub(crate) from: [f64; 2],
    pub(crate) to: [f64; 2],
    pub(crate) depth: i32,
    /// Fixed rotation in degrees; `None` is Auto.
    pub(crate) rotate: Option<f64>,
}

fn point(value: Option<&Value>) -> Option<[f64; 2]> {
    let list = value?.as_array()?;
    if list.len() != 2 {
        return None;
    }
    let mut out = [0.0; 2];
    for (slot, v) in out.iter_mut().zip(list) {
        let f = v.as_f64()?;
        if !f.is_finite() || f.abs() > MAX_COORDINATE {
            return None;
        }
        *slot = f;
    }
    Some(out)
}

/// The request's `pins`, each `{"from":[x,y],"to":[x,y],"depth":i,"rotate":deg|null}`
/// (`depth` 0 and `rotate` null when absent). Refuses by name.
pub(crate) fn pins(json: Option<&Value>) -> Result<Vec<Pin>, String> {
    let list = match json {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(list)) => list.as_slice(),
        Some(_) => return Err("Puppet Warp pins must be a list of pins.".into()),
    };
    if list.is_empty() {
        return Err("Puppet Warp needs at least one pin.".into());
    }
    if list.len() > MAX_PINS {
        return Err(format!("Puppet Warp takes at most {MAX_PINS} pins."));
    }
    list.iter()
        .enumerate()
        .map(|(i, value)| {
            let points = || {
                format!(
                    "Puppet Warp pin {i} needs from and to as two finite numbers within 1000000."
                )
            };
            let object = value.as_object().ok_or_else(points)?;
            if let Some(key) = object
                .keys()
                .find(|k| !matches!(k.as_str(), "from" | "to" | "depth" | "rotate"))
            {
                return Err(format!("Puppet Warp pin {i} has an unknown field `{key}`."));
            }
            let from = point(object.get("from")).ok_or_else(points)?;
            let to = point(object.get("to")).ok_or_else(points)?;
            let depth = match object.get("depth") {
                None | Some(Value::Null) => 0,
                Some(v) => whole(v, MAX_DEPTH).ok_or_else(|| {
                    format!("Puppet Warp pin {i} depth must be a whole number from -1000 to 1000.")
                })? as i32,
            };
            let rotate = match object.get("rotate") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| {
                    format!("Puppet Warp pin {i} rotate must be a finite angle in degrees or null.")
                })?),
            };
            Ok(Pin {
                from,
                to,
                depth,
                rotate,
            })
        })
        .collect()
}

/// Every pin unmoved and unrotated: the warp is the identity.
pub(crate) fn is_identity(pins: &[Pin]) -> bool {
    pins.iter()
        .all(|p| p.to == p.from && p.rotate.is_none_or(|r| r == 0.0))
}

/// Everything a request says besides its target: mode, mesh options, pins.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PuppetSpec {
    pub(crate) mode: Mode,
    pub(crate) options: MeshOptions,
    pub(crate) pins: Vec<Pin>,
}

impl PuppetSpec {
    /// The shared strict parse of the request fields; pins first.
    pub(crate) fn parse(
        mode: Option<&Value>,
        density: Option<&Value>,
        expansion: Option<&Value>,
        pin_list: Option<&Value>,
    ) -> Result<Self, String> {
        let pins = pins(pin_list)?;
        let mode = Mode::parse(mode)?;
        let options = MeshOptions::parse(density, expansion)?;
        Ok(Self {
            mode,
            options,
            pins,
        })
    }

    pub(crate) fn is_identity(&self) -> bool {
        is_identity(&self.pins)
    }
}

/// A snapped pin in a pose.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PosePin {
    pub(crate) vertex: usize,
    /// The vertex's rest position (the snapped `from`).
    pub(crate) from: [f64; 2],
    pub(crate) depth: i32,
}

/// A solved deformation of a mesh.
pub struct PuppetPose {
    mesh: PuppetMesh,
    positions: Vec<[f64; 2]>,
    pins: Vec<PosePin>,
}

/// Rest-triangle constants: area and the three basis gradients.
struct Element {
    area: f64,
    gradients: [[f64; 2]; 3],
}

fn element(r: [[f64; 2]; 3]) -> Element {
    let e1 = [r[1][0] - r[0][0], r[1][1] - r[0][1]];
    let e2 = [r[2][0] - r[0][0], r[2][1] - r[0][1]];
    let det = e1[0] * e2[1] - e2[0] * e1[1];
    // Rows of [e1 e2]⁻¹ are the gradients of the barycentric weights 1 and 2.
    let g1 = [e2[1] / det, -e2[0] / det];
    let g2 = [-e1[1] / det, e1[0] / det];
    Element {
        area: det.abs() / 2.0,
        gradients: [[-g1[0] - g2[0], -g1[1] - g2[1]], g1, g2],
    }
}

/// `J` of a triangle at `x` (its three deformed corners).
fn gradient(e: &Element, x: [[f64; 2]; 3]) -> [[f64; 2]; 2] {
    let mut j = [[0.0; 2]; 2];
    for (k, g) in e.gradients.iter().enumerate() {
        for c in 0..2 {
            for d in 0..2 {
                j[c][d] += x[k][c] * g[d];
            }
        }
    }
    j
}

/// The local step's target for `j`: see the module doc.
fn fit(j: [[f64; 2]; 2], mode: Mode, fixed: Option<f64>) -> [[f64; 2]; 2] {
    let (a, b, c, d) = (j[0][0], j[0][1], j[1][0], j[1][1]);
    let phi = fixed.unwrap_or_else(|| (c - b).atan2(a + d));
    let (sin, cos) = phi.sin_cos();
    let s = (cos * (a + d) + sin * (c - b)) / 2.0;
    let scale = match mode {
        Mode::Rigid => 1.0,
        Mode::Normal => (1.0 + s) / 2.0,
        Mode::Distort => s,
    };
    [[scale * cos, -scale * sin], [scale * sin, scale * cos]]
}

/// Union-find root with path halving.
fn root(parent: &mut [usize], mut v: usize) -> usize {
    while parent[v] != v {
        parent[v] = parent[parent[v]];
        v = parent[v];
    }
    v
}

/// Snap the pins (refusing by name) and solve; see the module doc.
pub(crate) fn solve(
    mesh: &PuppetMesh,
    pins: &[Pin],
    mode: Mode,
    job: JobContext<'_>,
) -> Result<PuppetPose, String> {
    let rest = mesh.vertices();
    let triangles = mesh.triangles();
    let n = rest.len();
    let mut pinned: Vec<Option<usize>> = vec![None; n];
    let mut snapped = Vec::with_capacity(pins.len());
    let mut positions = rest.to_vec();
    for (i, pin) in pins.iter().enumerate() {
        let v = mesh
            .snap(pin.from)
            .ok_or_else(|| format!("Puppet Warp pin {i} is outside the mesh."))?;
        if let Some(j) = pinned[v] {
            return Err(format!("Puppet Warp pins {j} and {i} share a mesh vertex."));
        }
        pinned[v] = Some(i);
        positions[v] = if pin.from == rest[v] {
            pin.to
        } else {
            [
                rest[v][0] + (pin.to[0] - pin.from[0]),
                rest[v][1] + (pin.to[1] - pin.from[1]),
            ]
        };
        snapped.push(PosePin {
            vertex: v,
            from: rest[v],
            depth: pin.depth,
        });
    }
    // Components: a vertex is free when it is unpinned and its component
    // holds a pin; everything else stays where it was put.
    let mut parent: Vec<usize> = (0..n).collect();
    for t in triangles {
        let a = root(&mut parent, t[0] as usize);
        for &v in &t[1..] {
            let b = root(&mut parent, v as usize);
            parent[b] = a;
        }
    }
    let mut anchored = vec![false; n];
    for p in &snapped {
        let r = root(&mut parent, p.vertex);
        anchored[r] = true;
    }
    let mut free = vec![usize::MAX; n];
    let mut count = 0;
    for v in 0..n {
        let r = root(&mut parent, v);
        if pinned[v].is_none() && anchored[r] {
            free[v] = count;
            count += 1;
        }
    }
    let elements: Vec<Element> = triangles
        .iter()
        .map(|t| element(t.map(|v| rest[v as usize])))
        .collect();
    // Fixed rotations: the lowest pin index wins a shared triangle.
    let fixed: Vec<Option<f64>> = triangles
        .iter()
        .map(|tri| {
            tri.iter()
                .filter_map(|&v| pinned[v as usize])
                .filter_map(|i| pins[i].rotate.map(|degrees| (i, degrees)))
                .min_by_key(|(i, _)| *i)
                .map(|(_, degrees)| degrees.to_radians())
        })
        .collect();
    if count == 0 {
        return Ok(PuppetPose {
            mesh: mesh.clone(),
            positions,
            pins: snapped,
        });
    }
    // Assemble K_ff and the constant fixed-vertex right-hand side.
    let mut lower = Vec::new();
    let mut constant = vec![[0.0f64; 2]; count];
    for (t, tri) in triangles.iter().enumerate() {
        job.every(t)?;
        let e = &elements[t];
        for a in 0..3 {
            let va = tri[a] as usize;
            let fa = free[va];
            if fa == usize::MAX {
                continue;
            }
            for (&vb, gb) in tri.iter().zip(&e.gradients) {
                let vb = vb as usize;
                let k = e.area * (e.gradients[a][0] * gb[0] + e.gradients[a][1] * gb[1]);
                let fb = free[vb];
                if fb == usize::MAX {
                    for (c, slot) in constant[fa].iter_mut().enumerate() {
                        *slot -= k * positions[vb][c];
                    }
                } else if fb <= fa {
                    lower.push((fa, fb, k));
                }
            }
        }
    }
    let envelope = factor(count, &lower, job)?;
    drop(lower);
    let identity_target = [[1.0, 0.0], [0.0, 1.0]];
    let mut targets = vec![identity_target; triangles.len()];
    let mut rhs = [vec![0.0f64; count], vec![0.0f64; count]];
    for iteration in 0..=MAX_ITERATIONS {
        job.check()?;
        if iteration > 0 {
            for (t, tri) in triangles.iter().enumerate() {
                job.every(t)?;
                let j = gradient(&elements[t], tri.map(|v| positions[v as usize]));
                targets[t] = fit(j, mode, fixed[t]);
            }
        }
        for (c, column) in rhs.iter_mut().enumerate() {
            for (slot, k) in column.iter_mut().zip(&constant) {
                *slot = k[c];
            }
        }
        for (t, tri) in triangles.iter().enumerate() {
            let e = &elements[t];
            for (a, &v) in tri.iter().enumerate() {
                let fa = free[v as usize];
                if fa == usize::MAX {
                    continue;
                }
                let g = e.gradients[a];
                for (c, column) in rhs.iter_mut().enumerate() {
                    column[fa] += e.area * (g[0] * targets[t][c][0] + g[1] * targets[t][c][1]);
                }
            }
        }
        envelope.solve_in_place(&mut rhs[0]);
        envelope.solve_in_place(&mut rhs[1]);
        let mut largest = 0.0f64;
        for v in 0..n {
            let f = free[v];
            if f == usize::MAX {
                continue;
            }
            let next = [rhs[0][f], rhs[1][f]];
            if !next[0].is_finite() || !next[1].is_finite() {
                return Err("the Puppet Warp solve did not converge".into());
            }
            largest = largest
                .max((next[0] - positions[v][0]).abs())
                .max((next[1] - positions[v][1]).abs());
            positions[v] = next;
        }
        if iteration > 0 && largest < TOLERANCE {
            break;
        }
    }
    Ok(PuppetPose {
        mesh: mesh.clone(),
        positions,
        pins: snapped,
    })
}

impl PuppetPose {
    pub(crate) fn mesh(&self) -> &PuppetMesh {
        &self.mesh
    }

    pub(crate) fn positions(&self) -> &[[f64; 2]] {
        &self.positions
    }

    pub(crate) fn pins(&self) -> &[PosePin] {
        &self.pins
    }

    /// `{"positions":[x0,y0,…],"pins":[{"vertex":k,"from":[x,y]}],"rect":[x,y,w,h]}`,
    /// `rect` the deformed vertices' outward-rounded box.
    pub(crate) fn describe(&self) -> Value {
        let flat: Vec<f64> = self.positions.iter().flatten().copied().collect();
        let pins: Vec<Value> = self
            .pins
            .iter()
            .map(|p| json!({"vertex": p.vertex, "from": p.from}))
            .collect();
        let r = outward_bounds(&self.positions);
        json!({"positions": flat, "pins": pins, "rect": [r.0, r.1, r.2, r.3]})
    }
}
