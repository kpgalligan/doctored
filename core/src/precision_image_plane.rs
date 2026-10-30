//! The private sample planes of `PrecisionImage` (the `#[path]` child module
//! `plane` of `precision_image`, 13B-3b-2). `Plane` and `Pixels`
//! are visible to `precision_image` and its two children only; the parent's
//! module doc is the representation contract. The plane-level rect read,
//! rect write and rebuild helpers here are what the writers in `build`
//! call.

use std::sync::Arc;

use crate::job_control::JobContext;
use crate::resources::Inventory;
use crate::tile_store::{RowSegments, Sample, TilePlane};

use super::{reserved_vec, ALLOCATION};

/// One sample plane in either representation (module doc).
#[derive(Clone)]
pub(super) enum Plane<S: Sample> {
    Tiled(TilePlane<S>),
    Contiguous(Arc<Vec<S>>),
}

#[derive(Clone)]
pub(super) enum Pixels {
    U16(Plane<[u16; 4]>),
    F32(Plane<[f32; 4]>),
}

impl<S: Sample> Plane<S> {
    /// The single sample at row-major `index` of a `width`-wide plane.
    /// Panics out of range, as a slice index does.
    pub(super) fn sample(&self, index: usize, width: u32) -> S {
        match self {
            Self::Tiled(plane) => {
                let width = width as usize;
                u32::try_from(index / width)
                    .ok()
                    .and_then(|y| plane.sample_at((index % width) as u32, y))
                    .expect("precision pixel index out of range")
            }
            Self::Contiguous(p) => p[index],
        }
    }

    /// Row `y`, columns `x0..x1` (exclusive), of a `width`×`height` plane as
    /// borrowed runs `(x, samples)`: `TilePlane::row_segments`'s rules
    /// verbatim (empty for `y >= height` or `x0 >= x1`, `x1` clamped to
    /// `width`). Tiled: one run per tile column; contiguous: ONE run `(x0,
    /// &p[y·w + x0 .. y·w + min(x1, w)])`. No allocation, no view.
    pub(super) fn row_segments(
        &self,
        (width, height): (u32, u32),
        y: u32,
        x0: u32,
        x1: u32,
    ) -> PlaneRows<'_, S> {
        match self {
            Self::Tiled(plane) => PlaneRows::Tiled(plane.row_segments(y, x0, x1)),
            Self::Contiguous(p) => {
                let x1 = x1.min(width);
                PlaneRows::Contiguous((y < height && x0 < x1).then(|| {
                    let row = y as usize * width as usize;
                    (x0, &p[row + x0 as usize..row + x1 as usize])
                }))
            }
        }
    }

    /// The sample at `(x, y)` of a `width`×`height` plane; `None` outside.
    /// No allocation, no view.
    pub(super) fn sample_at(&self, (width, height): (u32, u32), x: u32, y: u32) -> Option<S> {
        match self {
            Self::Tiled(plane) => plane.sample_at(x, y),
            Self::Contiguous(p) => {
                (x < width && y < height).then(|| p[y as usize * width as usize + x as usize])
            }
        }
    }

    /// An owned row-major copy through `reserved_vec`.
    pub(super) fn to_vec(&self) -> Result<Vec<S>, String> {
        match self {
            Self::Tiled(plane) => plane
                .to_contiguous(None)
                .map_err(|_| ALLOCATION.to_string()),
            Self::Contiguous(p) => {
                let mut out = reserved_vec(p.len())?;
                out.extend_from_slice(p);
                Ok(out)
            }
        }
    }

    /// The FFI lend's samples (`PrecisionImage::pinned_rgba16`): a tiled
    /// plane answers a PRIVATE row-major copy (a fresh `Arc`), a contiguous
    /// one its own buffer (`Arc::clone`, zero copy). No view cache, no job,
    /// no checkpoint. Err "could not allocate precision output" when the
    /// copy fails.
    pub(super) fn lent(&self) -> Result<Arc<Vec<S>>, String> {
        match self {
            Self::Tiled(plane) => plane
                .to_contiguous(None)
                .map(Arc::new)
                .map_err(|_| ALLOCATION.to_string()),
            Self::Contiguous(p) => Ok(Arc::clone(p)),
        }
    }

    /// Bitwise sample equality (`Sample::same_bits`); a mixed pair compares
    /// through the tiled side's `equals_contiguous`. No allocation, no job.
    pub(super) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Tiled(a), Self::Tiled(b)) => a.same_bits(b),
            (Self::Contiguous(a), Self::Contiguous(b)) => {
                Arc::ptr_eq(a, b)
                    || (a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.same_bits(*y)))
            }
            (Self::Tiled(t), Self::Contiguous(v)) | (Self::Contiguous(v), Self::Tiled(t)) => {
                t.equals_contiguous(v)
            }
        }
    }

    /// The pixel payload in bytes: `w·h·bpp` tiled, capacity contiguous.
    pub(super) fn storage_bytes(&self, width: u32, height: u32) -> u64 {
        let bpp = std::mem::size_of::<S>() as u64;
        match self {
            Self::Tiled(_) => u64::from(width) * u64::from(height) * bpp,
            Self::Contiguous(p) => p.capacity() as u64 * bpp,
        }
    }

    /// Tiled: the index as `tile_structure`, every tile as `precision_pixels`.
    /// Contiguous: 13B-3a's two records.
    pub(super) fn record(&self, inventory: &mut Inventory, cache: bool) {
        match self {
            Self::Tiled(plane) => plane.record_resources(inventory, "precision_pixels", cache),
            Self::Contiguous(p) => {
                if cache {
                    inventory.pin(p.clone());
                }
                inventory.object(&**p, "precision_buffer", cache);
                inventory.vec(p, "precision_pixels", cache);
            }
        }
    }

    /// The address the typed colour dedup keys on: the index allocation (as
    /// `TilePlane::record_resources` records it) or the Vec's buffer.
    pub(super) fn key(&self) -> usize {
        match self {
            Self::Tiled(plane) => plane.weak_index().as_ptr().cast::<()>() as usize,
            Self::Contiguous(p) => p.as_ptr() as usize,
        }
    }

    pub(super) fn is_tiled(&self) -> bool {
        matches!(self, Self::Tiled(_))
    }

    /// Tiles `self` and `other` share by pointer at the same index; 0 unless
    /// both are tiled (only the parent checks dimensions).
    pub(super) fn shared_tile_count(&self, other: &Self) -> usize {
        match (self, other) {
            (Self::Tiled(a), Self::Tiled(b)) => a.shared_tile_count(b),
            _ => 0,
        }
    }

    /// The bbox of the tile rects `self` and `other` do not share by pointer
    /// (both tiled over `dimensions`); `None` when all are shared or both
    /// hold the same contiguous `Arc`; else the whole plane.
    pub(super) fn unshared_rect(
        &self,
        other: &Self,
        (width, height): (u32, u32),
    ) -> Option<(u32, u32, u32, u32)> {
        let (a, b) = match (self, other) {
            (Self::Tiled(a), Self::Tiled(b)) if a.dimensions() == b.dimensions() => (a, b),
            (Self::Contiguous(a), Self::Contiguous(b)) if Arc::ptr_eq(a, b) => return None,
            _ => return Some((0, 0, width, height)),
        };
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        for ty in 0..a.rows() {
            for tx in 0..a.columns() {
                if a.tile(tx, ty)
                    .zip(b.tile(tx, ty))
                    .is_some_and(|(p, q)| Arc::ptr_eq(p, q))
                {
                    continue;
                }
                let (x, y, w, h) = a.tile_rect(tx, ty)?;
                bounds = Some(match bounds {
                    None => (x, y, x + w, y + h),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)),
                });
            }
        }
        bounds.map(|(x0, y0, x1, y1)| (x0, y0, x1 - x0, y1 - y0))
    }

    /// The tiles holding `samples` (a whole row-major `width`×`height`
    /// plane), sharing every bitwise-unchanged tile of `self`
    /// (`TilePlane::rebuilt_from`; `self`'s own plane when nothing changed).
    /// `None` when `self` is contiguous or of other dimensions, or the
    /// rebuild fails — the caller then tiles fresh. Polls nothing.
    pub(super) fn rebuilt_tiles(
        &self,
        width: u32,
        height: u32,
        samples: &[S],
    ) -> Option<TilePlane<S>> {
        let Self::Tiled(plane) = self else {
            return None;
        };
        if plane.dimensions() != (width, height) {
            return None;
        }
        match plane.rebuilt_from(samples, None) {
            Ok(Some(next)) => Some(next),
            Ok(None) => Some(plane.clone()),
            Err(_) => None,
        }
    }

    /// `TilePlane::with_rect` over either representation (`width`×`height`
    /// is the plane's own extent): `Err` on a sample-count mismatch, a rect
    /// leaving the plane or an allocation failure; `Ok(None)` for a zero
    /// area or no bitwise change. A contiguous plane is compared first, then
    /// its output is built straight into tiles at every size
    /// (`TilePlane::from_rows_with_job`: each row of `self` with the rect's
    /// span overlaid), so the write holds the new tiles plus one row and
    /// never a whole contiguous copy. Seeds no view.
    pub(super) fn with_rect(
        &self,
        (width, height): (u32, u32),
        (x, y, w, h): (u32, u32, u32, u32),
        samples: &[S],
    ) -> Result<Option<Self>, String> {
        let current = match self {
            Self::Tiled(plane) => {
                return Ok(plane.with_rect(x, y, w, h, samples, None)?.map(Self::Tiled));
            }
            Self::Contiguous(current) => current,
        };
        if samples.len() as u64 != u64::from(w) * u64::from(h) {
            return Err("tile plane sample count does not match dimensions".into());
        }
        if w == 0 || h == 0 {
            return Ok(None);
        }
        if !contains((width, height), (x, y, w, h)) {
            return Err("tile rect leaves the plane".into());
        }
        let (stride, span) = (width as usize, w as usize);
        let at = |row: usize| (y as usize + row) * stride + x as usize;
        let rows = samples.chunks_exact(span);
        if rows.enumerate().all(|(row, line)| {
            current[at(row)..at(row) + span]
                .iter()
                .zip(line)
                .all(|(a, b)| a.same_bits(*b))
        }) {
            return Ok(None);
        }
        let (top, bottom) = (y, y + h);
        let fill_row = |r: u32, out: &mut [S]| {
            let start = r as usize * stride;
            out.copy_from_slice(&current[start..start + stride]);
            if (top..bottom).contains(&r) {
                let line = (r - top) as usize * span;
                out[x as usize..x as usize + span].copy_from_slice(&samples[line..line + span]);
            }
            Ok(())
        };
        TilePlane::from_rows_with_job(width, height, fill_row, JobContext::NONE)
            .map(|plane| Some(Self::Tiled(plane)))
            .map_err(|_| ALLOCATION.to_string())
    }

    /// `TilePlane::read_rect` over either representation, row-major with
    /// stride `w`: `None` for a zero area, a rect leaving the plane or an
    /// allocation failure. A contiguous plane copies row slices. Binds no
    /// view.
    pub(super) fn read_rect(
        &self,
        (width, height): (u32, u32),
        (x, y, w, h): (u32, u32, u32, u32),
    ) -> Option<Vec<S>> {
        if w == 0 || h == 0 || !contains((width, height), (x, y, w, h)) {
            return None;
        }
        match self {
            Self::Tiled(plane) => plane.read_rect(x, y, w, h),
            Self::Contiguous(current) => {
                let mut out = reserved_vec(w as usize * h as usize).ok()?;
                for row in y as usize..(y + h) as usize {
                    let at = row * width as usize + x as usize;
                    out.extend_from_slice(&current[at..at + w as usize]);
                }
                Some(out)
            }
        }
    }
}

/// The runs of one row span of a [`Plane`] (`Plane::row_segments`).
pub(super) enum PlaneRows<'a, S: Sample> {
    Tiled(RowSegments<'a, S>),
    Contiguous(Option<(u32, &'a [S])>),
}

impl<'a, S: Sample> Iterator for PlaneRows<'a, S> {
    type Item = (u32, &'a [S]);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Tiled(runs) => runs.next(),
            Self::Contiguous(run) => run.take(),
        }
    }
}

/// A u16 sample as the normalized f32 every typed read answers: `f32::from(c)
/// / 65535.0` per component — the ONE u16 read rule (`PrecisionImage::pixel`,
/// `sample_at`, `PrecisionRun::pixel`).
pub(super) fn normalized16(sample: [u16; 4]) -> [f32; 4] {
    sample.map(|c| f32::from(c) / 65535.0)
}

fn contains((width, height): (u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    u64::from(x) + u64::from(w) <= u64::from(width)
        && u64::from(y) + u64::from(h) <= u64::from(height)
}

/// The tiles of an adopted contiguous plane at every size, or `None` only
/// when the tile store refuses it (the plane then stays contiguous). There
/// is no byte bound: each adopting constructor's plan
/// charges the tiling copy beside its adopted `Vec`.
pub(super) fn tiles_of<S: Sample>(width: u32, height: u32, samples: &[S]) -> Option<TilePlane<S>> {
    TilePlane::from_contiguous(width, height, samples, None).ok()
}

/// A bitwise-uniform adopted plane as ONE filled plane (`TilePlane::filled`:
/// one tile per shape) at every size, or `None` — no samples, a sample not
/// `Sample::same_bits` to the first (the scan stops there), or `filled`
/// refusing. Allocates nothing else and polls nothing (13B-3e-4b: a reopened
/// empty layer is filled again; later, above the cache bound too).
pub(super) fn uniform_tiles<S: Sample>(
    width: u32,
    height: u32,
    samples: &[S],
) -> Option<TilePlane<S>> {
    let first = *samples.first()?;
    if !samples.iter().all(|s| s.same_bits(first)) {
        return None;
    }
    TilePlane::filled(width, height, first).ok()
}
