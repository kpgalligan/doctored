//! Rect arithmetic for a selection-scoped layer-mask Invert: the
//! smallest rect holding both a mask's stored plane and the canvas, and a
//! stored plane re-laid into a larger one.
//!
//! A mask's stored plane covers only part of the canvas; everywhere else its
//! coverage is the constant `outside_coverage`. Inverting the whole mask
//! flips that constant, but inverting only the selected part cannot: selected
//! samples outside the stored rect must become stored samples first. So the
//! plane grows to the union of its rect and the canvas, filled with the
//! outside constant, which leaves the effective coverage of every canvas
//! sample unchanged, and the scoped write then runs over stored samples only.
//! Both depths (`doc_mask_invert`, `precision_scalar_edit`) share this module.

/// A stored plane's `(width, height)` and its canvas offset.
pub(crate) type PlaneRect = ((u32, u32), (i32, i32));

/// The smallest rect that holds both `plane` and the canvas rect
/// `(0, 0, canvas)`. `None` when either is empty or the union's origin or
/// size leaves the `i32` / `u32` range a plane offset and size are stored in.
pub(crate) fn union_with_canvas(plane: PlaneRect, canvas: (u32, u32)) -> Option<PlaneRect> {
    let ((w, h), (x, y)) = plane;
    if w == 0 || h == 0 || canvas.0 == 0 || canvas.1 == 0 {
        return None;
    }
    let left = i64::from(x).min(0);
    let top = i64::from(y).min(0);
    let right = (i64::from(x) + i64::from(w)).max(i64::from(canvas.0));
    let bottom = (i64::from(y) + i64::from(h)).max(i64::from(canvas.1));
    Some((
        (
            u32::try_from(right - left).ok()?,
            u32::try_from(bottom - top).ok()?,
        ),
        (i32::try_from(left).ok()?, i32::try_from(top).ok()?),
    ))
}

/// `samples` (row-major over `from`) re-laid into `to`, every sample `from`
/// does not cover set to `outside`. `None` when `samples` is not `from`'s
/// size, `from` is not inside `to`, or `to`'s sample count overflows.
pub(crate) fn grown<T: Copy>(
    samples: &[T],
    from: PlaneRect,
    to: PlaneRect,
    outside: T,
) -> Option<Vec<T>> {
    let ((fw, fh), (fx, fy)) = from;
    let ((tw, th), (tx, ty)) = to;
    let (fw, fh, tw, th) = (fw as usize, fh as usize, tw as usize, th as usize);
    if samples.len() != fw.checked_mul(fh)? {
        return None;
    }
    let dx = usize::try_from(i64::from(fx) - i64::from(tx)).ok()?;
    let dy = usize::try_from(i64::from(fy) - i64::from(ty)).ok()?;
    if dx.checked_add(fw)? > tw || dy.checked_add(fh)? > th {
        return None;
    }
    let mut out = vec![outside; tw.checked_mul(th)?];
    if fw > 0 {
        for (row, line) in samples.chunks_exact(fw).enumerate() {
            let at = (dy + row) * tw + dx;
            out[at..at + fw].copy_from_slice(line);
        }
    }
    Some(out)
}
