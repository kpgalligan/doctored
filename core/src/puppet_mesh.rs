//! Puppet Warp's mesh: the triangle mesh Edit ▸ Puppet Warp builds
//! over a layer's opaque pixels (or over the selection's float), its options,
//! and the point queries the pins use. Free functions and one value type over
//! a membership callback; `doc_puppet`, `precision_puppet` and
//! `selected_pixels_puppet` are the adapters.
//!
//! THE MASK. A pixel is in when the caller's `opaque` says so (alpha > 0, and
//! for a float coverage > 0 too). The mask is the opaque set's bounding box
//! grown by `|e| + 1` pixels (`e` the Expansion), so the plane always has an
//! outside ring, offset by `e` through `doc_select::offset_contour_with_context`
//! (the selection's Expand/Contract, one implementation of the contour
//! offset), and a byte of at least 128 counts as in. The offset keeps an
//! outside pixel whose centre lies within `e + ½` px of an opaque centre, so
//! a positive Expansion pads the mesh past anti-aliased edges and a negative
//! one pulls it in. Expansion 0 skips the offset: the SDF's 50% contour of a
//! binary mask is the mask itself, so the result is the same.
//!
//! THE GRID. With `m` the mask's pixel count and `t` the Density's target
//! cell count (Fewer Points 300, Normal 1000, More Points 3000), the spacing
//! is the integer `h = max(4, ⌈√(m / t)⌉)`: about `t` cells over a compact
//! shape, never finer than 4 px, because a cell smaller than the bicubic
//! kernel's reach buys no detail and only multiplies the solve. While the
//! mesh would have more than [`MAX_MESH_VERTICES`] vertices, `h` grows by a
//! quarter (rounded up), which bounds the solver's dense envelope however
//! thin or scattered the shape. Square cells of side `h` are anchored at the
//! MASK's bounding box (after the offset), so the float form and the mesh
//! export build the same mesh from the same pixels whatever rectangle the
//! caller scanned. A cell is kept when it holds a mask pixel; it splits along
//! its top-left–bottom-right diagonal when `i + j` is even and along the
//! other one when odd, the alternating pattern that keeps the mesh free of a
//! preferred shear direction. The kept cells' corners are the vertices,
//! numbered row by row; every mask pixel's centre lies strictly inside its
//! cell, so inside the mesh.
//!
//! LOCATE AND SNAP. [`PuppetMesh::locate`] answers the first rest triangle
//! (in index order) whose closed area holds a point, from the one to four
//! cells whose closed squares hold it; [`PuppetMesh::snap`] the nearest
//! vertex of that triangle, ties to the lower index.
use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc_select::offset_contour_with_context;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// A canvas rectangle `(x, y, w, h)`.
pub(crate) type Rect = (i64, i64, u32, u32);

/// The most vertices a mesh keeps; see the module doc. At 3000 the solver's
/// envelope is at most 36 MB however the vertices are ordered
/// (`puppet_arap::SOLVER_BYTES`), and a solve stays interactive.
pub(crate) const MAX_MESH_VERTICES: usize = 3000;
/// The finest cell side, in pixels; see the module doc.
const MIN_SPACING: u32 = 4;
/// The largest Expansion magnitude, in pixels.
pub(crate) const MAX_EXPANSION: i32 = 100;
/// Bookkeeping on top of [`mesh_plan_bytes`]'s per-pixel terms.
const MESH_BOOKKEEPING: u64 = 1024 * 1024;

const DENSITY_REFUSAL: &str = "Puppet Warp density must be fewer, normal or more.";
const EXPANSION_REFUSAL: &str =
    "Puppet Warp expansion must be a whole number of pixels from -100 to 100.";

/// The options bar's Density.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Density {
    Fewer,
    Normal,
    More,
}

impl Density {
    /// The target cell count; see the module doc.
    fn target(self) -> u64 {
        match self {
            Density::Fewer => 300,
            Density::Normal => 1000,
            Density::More => 3000,
        }
    }

    /// `"fewer" | "normal" | "more"`; absent or null is Normal.
    pub(crate) fn parse(value: Option<&Value>) -> Result<Self, String> {
        match value {
            None | Some(Value::Null) => Ok(Density::Normal),
            Some(Value::String(s)) => match s.as_str() {
                "fewer" => Ok(Density::Fewer),
                "normal" => Ok(Density::Normal),
                "more" => Ok(Density::More),
                _ => Err(DENSITY_REFUSAL.into()),
            },
            Some(_) => Err(DENSITY_REFUSAL.into()),
        }
    }
}

/// Density and Expansion: everything the mesh depends on besides the pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MeshOptions {
    pub(crate) density: Density,
    pub(crate) expansion: i32,
}

/// A whole number within `±limit`, or `None`. JSON writes `2` and `2.0` for
/// the same number, so an integral float is accepted.
pub(crate) fn whole(value: &Value, limit: i64) -> Option<i64> {
    let n = match value.as_i64() {
        Some(n) => n,
        None => {
            let f = value.as_f64()?;
            if !f.is_finite() || f.fract() != 0.0 || f.abs() > limit as f64 {
                return None;
            }
            f as i64
        }
    };
    // A range test, not `n.abs()`, which overflows on `i64::MIN`.
    (-limit..=limit).contains(&n).then_some(n)
}

impl MeshOptions {
    /// Density (default Normal) and Expansion (default 2 px).
    pub(crate) fn parse(
        density: Option<&Value>,
        expansion: Option<&Value>,
    ) -> Result<Self, String> {
        let density = Density::parse(density)?;
        let expansion = match expansion {
            None | Some(Value::Null) => 2,
            Some(v) => whole(v, i64::from(MAX_EXPANSION)).ok_or(EXPANSION_REFUSAL)? as i32,
        };
        Ok(Self { density, expansion })
    }

    /// The mesh exports' `{"density":…,"expansion":…}`, strict.
    pub(crate) fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("invalid puppet warp mesh options: {e}"))?;
        let object = value
            .as_object()
            .ok_or("puppet warp mesh options must be a JSON object")?;
        if let Some(key) = object
            .keys()
            .find(|k| !matches!(k.as_str(), "density" | "expansion"))
        {
            return Err(format!(
                "invalid puppet warp mesh options: unknown field `{key}`"
            ));
        }
        Self::parse(object.get("density"), object.get("expansion"))
    }

    fn grow(self) -> u32 {
        self.expansion.unsigned_abs() + 1
    }
}

/// The empty-mesh refusal.
pub(crate) fn empty_refusal(expansion: i32) -> String {
    format!("Puppet Warp needs pixels to mesh, and there are none at expansion {expansion} px.")
}

/// The working memory [`build`] can need for a scan of `rect`: the u8 mask
/// over `rect` grown by `|e| + 1`, the offset's two f32 distance planes when
/// `e ≠ 0`, the kept-cell and corner lists at the finest spacing (24 bytes
/// a cell), the
/// distance transform's line scratch, and bookkeeping. The mask is built over
/// the opaque bounding box, never larger than `rect`, so this bounds it.
pub(crate) fn mesh_plan_bytes(rect: Rect, options: &MeshOptions) -> Result<u64, String> {
    let g = u64::from(options.grow());
    let w = u64::from(rect.2) + 2 * g;
    let h = u64::from(rect.3) + 2 * g;
    let plane = w.checked_mul(h).ok_or("puppet warp mesh plan overflow")?;
    let distances = if options.expansion != 0 { plane * 8 } else { 0 };
    let cells = (w / u64::from(MIN_SPACING) + 2) * (h / u64::from(MIN_SPACING) + 2) * 24;
    let lines = w.max(h) * 24;
    Ok(plane
        .saturating_add(distances)
        .saturating_add(cells)
        .saturating_add(lines)
        .saturating_add(MESH_BOOKKEEPING))
}

/// The outward-rounded bounding box of `points` (at least 1 × 1).
pub(crate) fn outward_bounds(points: &[[f64; 2]]) -> Rect {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for p in points {
        for k in 0..2 {
            low[k] = low[k].min(p[k]);
            high[k] = high[k].max(p[k]);
        }
    }
    let x0 = low[0].floor() as i64;
    let y0 = low[1].floor() as i64;
    let x1 = (high[0].ceil() as i64).max(x0 + 1);
    let y1 = (high[1].ceil() as i64).max(y0 + 1);
    (
        x0,
        y0,
        u32::try_from(x1 - x0).unwrap_or(u32::MAX),
        u32::try_from(y1 - y0).unwrap_or(u32::MAX),
    )
}

struct MeshData {
    vertices: Vec<[f64; 2]>,
    triangles: Vec<[u32; 3]>,
    /// Canvas position of corner (0, 0).
    origin: [f64; 2],
    spacing: f64,
    /// Kept cells `(row, column)`, row-major; cell `k` owns triangles `2k`
    /// and `2k + 1`.
    cells: Vec<(u32, u32)>,
    rect: Rect,
}

/// A built mesh: cheap to clone (the pose keeps one).
#[derive(Clone)]
pub struct PuppetMesh {
    data: Arc<MeshData>,
}

/// Build the mesh of the opaque pixels of `rect` (`opaque(x, y)` in
/// rect-local pixels). Refuses an empty mask by name.
pub(crate) fn build(
    rect: Rect,
    opaque: impl Fn(u32, u32) -> bool,
    options: &MeshOptions,
    job: JobContext<'_>,
) -> Result<PuppetMesh, String> {
    // Pass 1: the opaque bounding box, rect-local, inclusive.
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in 0..rect.3 {
        job.check()?;
        let mut first = None;
        let mut last = 0;
        for x in 0..rect.2 {
            job.every(x as usize)?;
            if opaque(x, y) {
                first.get_or_insert(x);
                last = x;
            }
        }
        if let Some(first) = first {
            found = Some(match found {
                None => (first, y, last, y),
                Some((x0, y0, x1, _)) => (x0.min(first), y0, x1.max(last), y),
            });
        }
    }
    let (bx0, by0, bx1, by1) = found.ok_or_else(|| empty_refusal(options.expansion))?;
    let g = options.grow();
    let pw = (bx1 - bx0 + 1) as usize + 2 * g as usize;
    let ph = (by1 - by0 + 1) as usize + 2 * g as usize;
    // Pass 2: the u8 mask over the box grown by g.
    let mut mask: Vec<u8> = reserved_vec(pw * ph)?;
    for py in 0..ph {
        job.check()?;
        let ly = i64::from(by0) + py as i64 - i64::from(g);
        for px in 0..pw {
            let lx = i64::from(bx0) + px as i64 - i64::from(g);
            let inside = lx >= 0
                && ly >= 0
                && lx < i64::from(rect.2)
                && ly < i64::from(rect.3)
                && opaque(lx as u32, ly as u32);
            mask.push(if inside { 255 } else { 0 });
        }
    }
    if options.expansion != 0 {
        offset_contour_with_context(&mut mask, pw, ph, options.expansion as f32, job)?;
    }
    // The offset mask's bounding box and count, plane-local.
    let mut count = 0u64;
    let (mut mx0, mut my0, mut mx1, mut my1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..ph {
        job.check()?;
        for x in 0..pw {
            if mask[y * pw + x] >= 128 {
                count += 1;
                mx0 = mx0.min(x);
                mx1 = mx1.max(x);
                my0 = my0.min(y);
                my1 = my1.max(y);
            }
        }
    }
    if count == 0 {
        return Err(empty_refusal(options.expansion));
    }
    let inside = |x: usize, y: usize| mask[y * pw + x] >= 128;
    let target = options.density.target();
    let mut h = ((count as f64 / target as f64).sqrt().ceil() as u32).max(MIN_SPACING);
    let (cells, corners) = loop {
        let cells = kept_cells(h as usize, (mx0, my0, mx1, my1), &inside, job)?;
        let corners = corner_rows(&cells, ((my1 - my0) / h as usize + 1) as u32);
        let total: usize = corners.iter().map(|(_, cols)| cols.len()).sum();
        if total <= MAX_MESH_VERTICES {
            break (cells, corners);
        }
        h = (h * 5).div_ceil(4);
    };
    drop(mask);
    let origin = [
        (rect.0 + i64::from(bx0) - i64::from(g) + mx0 as i64) as f64,
        (rect.1 + i64::from(by0) - i64::from(g) + my0 as i64) as f64,
    ];
    Ok(assemble(origin, f64::from(h), cells, &corners))
}

/// The full grid over `rect` (Perspective Warp's extension): every
/// cell kept, square cells of side `h = max(4, ⌈√(area / t)⌉)` (`t` the
/// Density's target) anchored at the rect's top-left and grown by a quarter
/// while the grid would pass [`MAX_MESH_VERTICES`], split by [`build`]'s
/// rule. The last row and column of cells may overhang the rect.
pub(crate) fn grid(rect: Rect, density: Density) -> Result<PuppetMesh, String> {
    if rect.2 == 0 || rect.3 == 0 {
        return Err("a mesh grid needs a non-empty rect".into());
    }
    let count = u64::from(rect.2) * u64::from(rect.3);
    let mut h = ((count as f64 / density.target() as f64).sqrt().ceil() as u32).max(MIN_SPACING);
    let (columns, rows) = loop {
        let columns = rect.2.div_ceil(h);
        let rows = rect.3.div_ceil(h);
        if (columns as usize + 1) * (rows as usize + 1) <= MAX_MESH_VERTICES {
            break (columns, rows);
        }
        h = (h * 5).div_ceil(4);
    };
    let cells: Vec<(u32, u32)> = (0..rows)
        .flat_map(|j| (0..columns).map(move |i| (j, i)))
        .collect();
    let corners = corner_rows(&cells, rows);
    Ok(assemble(
        [rect.0 as f64, rect.1 as f64],
        f64::from(h),
        cells,
        &corners,
    ))
}

/// The vertices (row by row) and the two triangles of each kept cell, split
/// by the module doc's alternating rule: [`build`]'s and [`grid`]'s shared
/// assembly.
fn assemble(
    origin: [f64; 2],
    spacing: f64,
    cells: Vec<(u32, u32)>,
    corners: &[(usize, Vec<u32>)],
) -> PuppetMesh {
    let mut vertices = Vec::new();
    for (row, (_, cols)) in corners.iter().enumerate() {
        for &col in cols {
            vertices.push([
                origin[0] + f64::from(col) * spacing,
                origin[1] + row as f64 * spacing,
            ]);
        }
    }
    let index = |row: u32, col: u32| -> u32 {
        let (base, cols) = &corners[row as usize];
        let at = cols.binary_search(&col).expect("a kept cell's corner");
        (*base + at) as u32
    };
    let mut triangles = Vec::with_capacity(cells.len() * 2);
    for &(j, i) in &cells {
        let tl = index(j, i);
        let tr = index(j, i + 1);
        let bl = index(j + 1, i);
        let br = index(j + 1, i + 1);
        if (i + j).is_multiple_of(2) {
            triangles.push([tl, tr, br]);
            triangles.push([tl, br, bl]);
        } else {
            triangles.push([tl, tr, bl]);
            triangles.push([tr, br, bl]);
        }
    }
    let rect = outward_bounds(&vertices);
    PuppetMesh {
        data: Arc::new(MeshData {
            vertices,
            triangles,
            origin,
            spacing,
            cells,
            rect,
        }),
    }
}

/// The kept cells at spacing `h` over the mask box (plane-local, inclusive),
/// row-major.
fn kept_cells(
    h: usize,
    (mx0, my0, mx1, my1): (usize, usize, usize, usize),
    inside: &impl Fn(usize, usize) -> bool,
    job: JobContext<'_>,
) -> Result<Vec<(u32, u32)>, String> {
    let ncx = (mx1 - mx0) / h + 1;
    let ncy = (my1 - my0) / h + 1;
    let mut cells = Vec::new();
    let mut hit = vec![false; ncx];
    for j in 0..ncy {
        job.check()?;
        hit.iter_mut().for_each(|v| *v = false);
        let y0 = my0 + j * h;
        let y1 = (y0 + h - 1).min(my1);
        for y in y0..=y1 {
            for x in mx0..=mx1 {
                if inside(x, y) {
                    hit[(x - mx0) / h] = true;
                }
            }
        }
        for (i, &kept) in hit.iter().enumerate() {
            if kept {
                cells
                    .try_reserve(1)
                    .map_err(|_| "could not allocate the puppet warp mesh")?;
                cells.push((j as u32, i as u32));
            }
        }
    }
    Ok(cells)
}

/// Each corner row's first vertex index and its sorted columns.
fn corner_rows(cells: &[(u32, u32)], rows: u32) -> Vec<(usize, Vec<u32>)> {
    let mut out: Vec<(usize, Vec<u32>)> = (0..=rows).map(|_| (0, Vec::new())).collect();
    for &(j, i) in cells {
        for r in [j, j + 1] {
            let cols = &mut out[r as usize].1;
            cols.push(i);
            cols.push(i + 1);
        }
    }
    let mut base = 0;
    for (start, cols) in &mut out {
        cols.sort_unstable();
        cols.dedup();
        *start = base;
        base += cols.len();
    }
    out
}

impl PuppetMesh {
    pub(crate) fn vertices(&self) -> &[[f64; 2]] {
        &self.data.vertices
    }

    pub(crate) fn triangles(&self) -> &[[u32; 3]] {
        &self.data.triangles
    }

    /// The cell side in pixels.
    pub(crate) fn spacing(&self) -> f64 {
        self.data.spacing
    }

    /// The first rest triangle (index order) whose closed area holds `p`.
    pub(crate) fn locate(&self, p: [f64; 2]) -> Option<usize> {
        let d = &self.data;
        let fx = (p[0] - d.origin[0]) / d.spacing;
        let fy = (p[1] - d.origin[1]) / d.spacing;
        if !fx.is_finite() || !fy.is_finite() {
            return None;
        }
        let (ix, iy) = (fx.floor(), fy.floor());
        let columns = if fx == ix { [ix - 1.0, ix] } else { [ix, ix] };
        let rows = if fy == iy { [iy - 1.0, iy] } else { [iy, iy] };
        let mut best: Option<usize> = None;
        for &j in &rows {
            for &i in &columns {
                if i < 0.0 || j < 0.0 || i > f64::from(u32::MAX) || j > f64::from(u32::MAX) {
                    continue;
                }
                let key = (j as u32, i as u32);
                let Ok(k) = d.cells.binary_search(&key) else {
                    continue;
                };
                let (u, v) = (fx - i, fy - j);
                let even = (key.0 + key.1).is_multiple_of(2);
                let (first, second) = if even {
                    (v <= u, u <= v)
                } else {
                    (u + v <= 1.0, u + v >= 1.0)
                };
                let hit = if first {
                    Some(2 * k)
                } else if second {
                    Some(2 * k + 1)
                } else {
                    None
                };
                if let Some(t) = hit {
                    best = Some(best.map_or(t, |b| b.min(t)));
                }
            }
        }
        best
    }

    /// The nearest vertex of [`Self::locate`]'s triangle, ties to the lower
    /// index.
    pub(crate) fn snap(&self, p: [f64; 2]) -> Option<usize> {
        let t = self.locate(p)?;
        let mut best: Option<(f64, usize)> = None;
        for &v in &self.data.triangles[t] {
            let q = self.data.vertices[v as usize];
            let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
            let better = match best {
                None => true,
                Some((bd, bv)) => d < bd || d == bd && (v as usize) < bv,
            };
            if better {
                best = Some((d, v as usize));
            }
        }
        best.map(|(_, v)| v)
    }

    /// `{"vertices":[[x,y]],"triangles":[[a,b,c]],"rect":[x,y,w,h]}`.
    pub(crate) fn describe(&self) -> Value {
        let d = &self.data;
        json!({
            "vertices": d.vertices,
            "triangles": d.triangles,
            "rect": [d.rect.0, d.rect.1, d.rect.2, d.rect.3],
        })
    }
}
