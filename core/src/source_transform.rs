//! Accumulated canvas mapping of an immutable retained image master. Affine
//! bindings preserve their original six-number native and JSON representation;
//! projective bindings use a normalized row-major homography.
//!
//! accumulated affine or projective maps always sample original retained masters, while masks ride
//! the incremental canvas map once. Whole-document geometry preserves bindings at both depths.
//! `source_info` returns `transform_kind`, with `transform:null` and `projective_transform` for
//! projective bindings. Missing linked files are never read during reopen.

use crate::doc_perspective::{transformed_projective_bounds, Homography};
use crate::doc_transform::{transformed_plane_bounds, Affine};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SourceTransform {
    Affine([f64; 6]),
    Projective([f64; 9]),
}

impl SourceTransform {
    pub(crate) fn matrix(self) -> [f64; 9] {
        match self {
            Self::Affine([a, b, c, d, tx, ty]) => [a, c, tx, b, d, ty, 0., 0., 1.],
            Self::Projective(matrix) => matrix,
        }
    }

    pub(crate) fn inverse(self) -> Result<[f64; 9], String> {
        let matrix = self.matrix();
        Homography(matrix)
            .visible_inverse()
            .filter(|_| {
                Homography(matrix).determinant().abs() >= crate::doc_transform::MIN_DETERMINANT
            })
            .ok_or_else(|| "source transform is nonfinite or singular".into())
    }
}

pub(crate) fn bounds(
    dim: (u32, u32),
    transform: SourceTransform,
) -> Result<(i32, i32, u32, u32), String> {
    let result = match transform {
        SourceTransform::Affine(matrix) => {
            let matrix = Affine::from_array(matrix);
            if !matrix.is_finite() || matrix.inverse().is_none() {
                return Err("source transform is nonfinite or singular".into());
            }
            transformed_plane_bounds(dim, (0, 0), &matrix)
        }
        SourceTransform::Projective(matrix) => {
            SourceTransform::Projective(matrix).inverse()?;
            transformed_projective_bounds(dim, (0, 0), &matrix)
        }
    };
    result.ok_or_else(|| "source transform crosses the horizon or exceeds supported bounds".into())
}

pub(crate) fn compose(
    after: SourceTransform,
    before: SourceTransform,
) -> Result<SourceTransform, String> {
    if let (SourceTransform::Affine(a), SourceTransform::Affine(b)) = (after, before) {
        let matrix = crate::retained_source::compose(a, b);
        crate::retained_source::validate_matrix(matrix)?;
        return Ok(SourceTransform::Affine(matrix));
    }
    let (a, b) = (after.matrix(), before.matrix());
    let mut result = [0.; 9];
    for row in 0..3 {
        for col in 0..3 {
            result[row * 3 + col] = (0..3).map(|i| a[row * 3 + i] * b[i * 3 + col]).sum();
        }
    }
    if !result.iter().all(|v| v.is_finite()) || result[8].abs() < 1e-12 {
        return Err("composed source transform is nonfinite or singular".into());
    }
    let scale = result[8];
    for value in &mut result {
        *value /= scale;
    }
    SourceTransform::Projective(result).inverse()?;
    Ok(SourceTransform::Projective(result))
}

pub(crate) fn translated(
    transform: SourceTransform,
    dx: f64,
    dy: f64,
) -> Result<SourceTransform, String> {
    compose(SourceTransform::Affine([1., 0., 0., 1., dx, dy]), transform)
}
