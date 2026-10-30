//! The FILLED constructors of `PrecisionImage` (the `#[path]` child module
//! `fill` of `precision_image`, 13B-3d-3b): a plane of one repeated sample
//! built over `TilePlane::filled`, so it retains one tile per tile shape
//! (≤ 4) plus the index and no view is bound, seeded or cached. That
//! holds at every size, above `CONTIGUOUS_CACHE_BYTES` too: a
//! plane stays contiguous only when the tile store refuses it. Nothing is
//! polled and nothing is admitted (a filled plane retains ≤ 4 tiles), so
//! the refusals are `pixel_count`'s sentence, `EXTENDED` and `ALLOCATION`.
//! [`PrecisionImage::from_rgba16_tiles_decoded`] adopts an already-decoded
//! tiled plane (the banded reopen) and admits it as the decoded
//! constructors do, and [`PrecisionImage::from_rgba32_tiles_decoded`] is its
//! f32 twin (the float `.rz` reopen and the float TIFF open
//! decode straight into tiles at every size);
//! [`PrecisionImage::from_rgba16_tiles_unseeded`] adopts a built plane's
//! tiles as they are (the uniform resample output, whose uniform
//! tiles are already shared) under the same admission.

use std::sync::Arc;

use crate::icc::IccProfile;
use crate::tile_store::{Sample, TilePlane};

use super::build::EXTENDED;
use super::plane::{Pixels, Plane};
use super::{
    admit, extended_pixel, pixel_count, process_working_limit, reserved_vec, PrecisionImage,
    ALLOCATION, BOOKKEEPING_BYTES,
};

impl PrecisionImage {
    /// A `width`×`height` RGBA16 image of `value` everywhere: a fresh
    /// identity, no pinned view, nothing seeded. Public only as a test seam
    /// (`tests/native_file_size.rs` builds 100 MP layers of distinct fills
    /// on one profile handle without allocating them).
    #[doc(hidden)]
    pub fn filled_rgba16(
        width: u32,
        height: u32,
        value: [u16; 4],
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        let plane = filled_plane(width, height, value)?;
        Ok(Self::assembled(width, height, profile, Pixels::U16(plane)))
    }

    /// Adopts decoded tiles (the banded native reopen): a fresh identity,
    /// nothing seeded, and a bitwise-uniform plane becomes ONE filled plane
    /// (`Tiling::Decoded`'s rule, 13B-3e-4b). Admits the plane's `8·w·h`
    /// bytes beside bookkeeping first, as `from_rgba16_decoded` does.
    pub(crate) fn from_rgba16_tiles_decoded(
        plane: TilePlane<[u16; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        let (width, height) = plane.dimensions();
        let bytes = pixel_count(width, height)? as u64 * 8;
        admit(bytes, BOOKKEEPING_BYTES, 0, process_working_limit())?;
        let plane = match plane.uniform_sample() {
            Some(value) => TilePlane::filled(width, height, value)?,
            None => plane,
        };
        Ok(Self::assembled(
            width,
            height,
            profile,
            Pixels::U16(Plane::Tiled(plane)),
        ))
    }

    /// Adopts `plane`'s tiles as they are (the uniform resample):
    /// a fresh identity, nothing seeded, no uniform refill. Admits the
    /// plane's `8·w·h` bytes beside bookkeeping first, as
    /// [`Self::from_rgba16_tiles_decoded`] does.
    pub(crate) fn from_rgba16_tiles_unseeded(
        plane: TilePlane<[u16; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        let (width, height) = plane.dimensions();
        let bytes = pixel_count(width, height)? as u64 * 8;
        admit(bytes, BOOKKEEPING_BYTES, 0, process_working_limit())?;
        Ok(Self::assembled(
            width,
            height,
            profile,
            Pixels::U16(Plane::Tiled(plane)),
        ))
    }

    /// The f32 twin of [`Self::from_rgba16_tiles_decoded`] (the float `.rz`
    /// raster and the float TIFF open decode into tiles, and float paint's
    /// grown-extent arm builds its plane into tiles).
    /// Admits the plane's `16·w·h` bytes beside bookkeeping first, then
    /// checks every sample with `extended_pixel` through
    /// `TilePlane::row_segments` (no allocation) and refuses the first
    /// failure with `from_rgba32_extended`'s sentence. A bitwise-uniform
    /// plane becomes ONE filled plane. A fresh identity, nothing seeded, no
    /// job.
    pub(crate) fn from_rgba32_tiles_decoded(
        plane: TilePlane<[f32; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        let (width, height) = plane.dimensions();
        let bytes = pixel_count(width, height)? as u64 * 16;
        admit(bytes, BOOKKEEPING_BYTES, 0, process_working_limit())?;
        for y in 0..height {
            for (_, run) in plane.row_segments(y, 0, width) {
                if !run.iter().all(|pixel| extended_pixel(*pixel)) {
                    return Err(EXTENDED.into());
                }
            }
        }
        let plane = match plane.uniform_sample() {
            Some(value) => TilePlane::filled(width, height, value)?,
            None => plane,
        };
        Ok(Self::assembled(
            width,
            height,
            profile,
            Pixels::F32(Plane::Tiled(plane)),
        ))
    }

    /// The extended-range f32 twin of [`Self::filled_rgba16`]; refuses a
    /// `value` failing `extended_pixel` with `from_rgba32_extended`'s
    /// sentence before allocating.
    pub(crate) fn filled_rgba32_extended(
        width: u32,
        height: u32,
        value: [f32; 4],
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        if !extended_pixel(value) {
            return Err(EXTENDED.into());
        }
        let plane = filled_plane(width, height, value)?;
        Ok(Self::assembled(width, height, profile, Pixels::F32(plane)))
    }
}

/// One plane of `value`: tiled through `TilePlane::filled` at every size,
/// contiguous only when the store refuses it.
fn filled_plane<S: Sample>(width: u32, height: u32, value: S) -> Result<Plane<S>, String> {
    let count = pixel_count(width, height)?;
    if let Ok(plane) = TilePlane::filled(width, height, value) {
        return Ok(Plane::Tiled(plane));
    }
    let mut samples = reserved_vec(count).map_err(|_| ALLOCATION.to_string())?;
    samples.resize(count, value);
    Ok(Plane::Contiguous(Arc::new(samples)))
}
