//! Puppet Warp's linear solver: a reverse Cuthill–McKee ordering and
//! an envelope (skyline) Cholesky factor of the symmetric positive definite
//! system the ARAP global step solves (`puppet_arap`). Free functions over
//! plain triplets.
//!
//! WHY AN ENVELOPE. The system is the mesh's cotangent Laplacian with the
//! pinned vertices eliminated: at most `puppet_mesh::MAX_MESH_VERTICES`
//! unknowns, a handful of neighbours each. Reverse Cuthill–McKee numbers a
//! grid-like mesh so every row's nonzeros sit within about a row of cells of
//! the diagonal, and a Cholesky factor fills in only inside that envelope, so
//! the factor costs about `n·b²` and each solve `n·b` for a bandwidth `b` of
//! tens. The worst case is the dense triangle, which `SOLVER_BYTES` charges.
//! The factor is computed once per pinned set and reused by every iteration's
//! two solves (x and y share the matrix).
//!
//! RCM here starts each connected component at its lowest-degree vertex
//! (ties to the lower index) and visits neighbours in ascending (degree,
//! index), then reverses the whole order: deterministic, so a preview and a
//! commit factor the same matrix identically.
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// A lower-triangular Cholesky factor `L` with `P·A·Pᵀ = L·Lᵀ`, stored by
/// rows from each row's first nonzero column to the diagonal.
pub(crate) struct Envelope {
    /// `perm[new] = old`.
    perm: Vec<usize>,
    /// Each permuted row's first stored column.
    first: Vec<usize>,
    /// Each permuted row's offset into `values`.
    start: Vec<usize>,
    values: Vec<f64>,
}

const NOT_DEFINITE: &str = "the Puppet Warp mesh system is not positive definite";

/// Factor the `n × n` symmetric matrix whose lower triangle (row ≥ column) is
/// the sum of `lower`'s entries (repeats add).
pub(crate) fn factor(
    n: usize,
    lower: &[(usize, usize, f64)],
    job: JobContext<'_>,
) -> Result<Envelope, String> {
    let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(i, j, _) in lower {
        if i >= n || j >= n {
            return Err("puppet warp system entry out of range".into());
        }
        if i != j {
            neighbours[i].push(j);
            neighbours[j].push(i);
        }
    }
    for list in &mut neighbours {
        list.sort_unstable();
        list.dedup();
    }
    let perm = reverse_cuthill_mckee(&neighbours);
    let mut inverse = vec![0usize; n];
    for (new, &old) in perm.iter().enumerate() {
        inverse[old] = new;
    }
    let mut first: Vec<usize> = (0..n).collect();
    for (old, list) in neighbours.iter().enumerate() {
        let row = inverse[old];
        for &other in list {
            let col = inverse[other];
            if col < row {
                first[row] = first[row].min(col);
            }
        }
    }
    let mut start = Vec::with_capacity(n);
    let mut total = 0usize;
    for (row, &f) in first.iter().enumerate() {
        start.push(total);
        total += row - f + 1;
    }
    let mut values: Vec<f64> = reserved_vec(total)?;
    values.resize(total, 0.0);
    for &(i, j, v) in lower {
        let (a, b) = (inverse[i], inverse[j]);
        let (row, col) = if a >= b { (a, b) } else { (b, a) };
        values[start[row] + col - first[row]] += v;
    }
    for i in 0..n {
        job.check()?;
        let fi = first[i];
        for j in fi..i {
            let fj = first[j];
            let k0 = fi.max(fj);
            let row_i = &values[start[i] + k0 - fi..start[i] + j - fi];
            let row_j = &values[start[j] + k0 - fj..start[j] + j - fj];
            let dot: f64 = row_i.iter().zip(row_j).map(|(a, b)| a * b).sum();
            let diagonal = values[start[j] + j - fj];
            let at = start[i] + j - fi;
            values[at] = (values[at] - dot) / diagonal;
        }
        let row = &values[start[i]..start[i] + i - fi];
        let squares: f64 = row.iter().map(|v| v * v).sum();
        let at = start[i] + i - fi;
        let d = values[at] - squares;
        if !(d > 0.0 && d.is_finite()) {
            return Err(NOT_DEFINITE.into());
        }
        values[at] = d.sqrt();
    }
    Ok(Envelope {
        perm,
        first,
        start,
        values,
    })
}

/// The RCM order (`perm[new] = old`); see the module doc.
fn reverse_cuthill_mckee(neighbours: &[Vec<usize>]) -> Vec<usize> {
    let n = neighbours.len();
    let degree = |v: usize| neighbours[v].len();
    let mut by_degree: Vec<usize> = (0..n).collect();
    by_degree.sort_by_key(|&v| (degree(v), v));
    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut next = Vec::new();
    for &seed in &by_degree {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        let mut head = order.len();
        order.push(seed);
        while head < order.len() {
            let v = order[head];
            head += 1;
            next.clear();
            next.extend(neighbours[v].iter().copied().filter(|&u| !visited[u]));
            next.sort_by_key(|&u| (degree(u), u));
            for &u in &next {
                visited[u] = true;
                order.push(u);
            }
        }
    }
    order.reverse();
    order
}

impl Envelope {
    /// Solve `A·x = b` in place.
    pub(crate) fn solve_in_place(&self, b: &mut [f64]) {
        let n = self.perm.len();
        let mut y: Vec<f64> = self.perm.iter().map(|&old| b[old]).collect();
        let at = |row: usize, col: usize| self.start[row] + col - self.first[row];
        for i in 0..n {
            let fi = self.first[i];
            let row = &self.values[self.start[i]..at(i, i)];
            let dot: f64 = row.iter().zip(&y[fi..i]).map(|(a, b)| a * b).sum();
            y[i] = (y[i] - dot) / self.values[at(i, i)];
        }
        for i in (0..n).rev() {
            y[i] /= self.values[at(i, i)];
            let yi = y[i];
            let fi = self.first[i];
            let row = &self.values[self.start[i]..at(i, i)];
            for (slot, l) in y[fi..i].iter_mut().zip(row) {
                *slot -= l * yi;
            }
        }
        for (new, &old) in self.perm.iter().enumerate() {
            b[old] = y[new];
        }
    }
}
