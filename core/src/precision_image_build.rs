//! The constructors and writers of `PrecisionImage` (the `#[path]` child
//! module `build` of `precision_image`, 13B-3b-2). Every constructor keeps
//! 13B-3a's statement order over the adopted `Vec` (admission, reservation
//! and validation read that buffer) and only then tiles it; the tiling step
//! is the one thing the variants differ in (`Tiling`). The writer contract
//! (the byte `LayerRaster`'s, 13B-2b) is the parent's module doc. The
//! filled constructors (no adopted `Vec`) are the sibling `fill` (13B-3d-3b);
//! a DECODED bitwise-uniform plane is filled too (`Tiling::Decoded`,
//! 13B-3e-4b).

use std::sync::{Arc, OnceLock};

use crate::contiguous_views::{self, PlaneKey, View};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::tile_store::TILE_EDGE;

use super::plane::{tiles_of, uniform_tiles, Pixels, Plane};
use super::{
    admit, extended_pixel, pixel_count, process_working_limit, reserved_vec, PrecisionImage,
    PrecisionStorage, BOOKKEEPING_BYTES,
};

/// The extended-range refusal of `from_rgba32_extended` and its writers.
pub(super) const EXTENDED: &str =
    "32-bit float samples must be finite within ±65504 with alpha in [0,1]";

/// A finite unit value as a stored u16 code: clamped to [0, 1], then
/// `round(v · 65535)`. The ONE statement of the rule a render's samples are
/// stored by at u16 (`float_samples`), which the float layer thumbnail's box
/// pass also reads (`precision_thumbnail`).
pub(crate) fn unit_to_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// How a constructor turns its adopted contiguous plane into the stored one.
#[derive(Clone, Copy)]
enum Tiling<'a> {
    /// `tile()`: fresh tiles, an adopted u16 buffer seeds the view cache
    /// (an f32 one seeds nothing since 13B-3e-4c-2a).
    Seeded,
    /// `tile_unseeded()`: fresh tiles, nothing cached.
    Unseeded,
    /// `tile_decoded()`: a bitwise-uniform plane filled, else `Unseeded`
    /// (13B-3e-4b).
    Decoded,
    /// Shares every bitwise-unchanged tile of the image (`rebuilt_*`),
    /// never seeding; else `Unseeded` (13B-3d-3a).
    Rebuilt(&'a PrecisionImage),
    /// Leaves the plane contiguous, as a tile-store refusal does: the test
    /// seam `contiguous_rgba32_extended` only.
    Kept,
}

impl PrecisionImage {
    /// A fresh, uniquely identified image over `pixels` (no pinned view, no
    /// alpha facts known).
    pub(super) fn assembled(w: u32, h: u32, profile: Arc<IccProfile>, pixels: Pixels) -> Self {
        Self {
            width: w,
            height: h,
            profile,
            pixels,
            identity: Arc::new(()),
            pinned: OnceLock::new(),
            alpha: Arc::new(OnceLock::new()),
        }
    }

    /// A contiguous u16 plane → Tiled, seeding the view cache with the
    /// adopted buffer, when the tile store accepts it; otherwise it stays
    /// contiguous (no seed). Any other plane is [`Self::tile_unseeded`]: no f32 plane seeds
    /// since 13B-3e-4c-2a. Identity is unchanged and no job is polled.
    fn tile(&mut self) {
        let (width, height) = (self.width, self.height);
        let Pixels::U16(Plane::Contiguous(samples)) = &self.pixels else {
            return self.tile_unseeded();
        };
        if let Some(plane) = tiles_of(width, height, samples) {
            contiguous_views::seed(
                PlaneKey::Rgba16(plane.weak_index()),
                View::Rgba16(Arc::clone(samples)),
            );
            self.pixels = Pixels::U16(Plane::Tiled(plane));
        }
    }

    /// [`Self::tile`] without the seed: the same `tiles_of`, nothing cached.
    fn tile_unseeded(&mut self) {
        let (width, height) = (self.width, self.height);
        match &self.pixels {
            Pixels::U16(Plane::Contiguous(samples)) => {
                if let Some(plane) = tiles_of(width, height, samples) {
                    self.pixels = Pixels::U16(Plane::Tiled(plane));
                }
            }
            Pixels::F32(Plane::Contiguous(samples)) => {
                if let Some(plane) = tiles_of(width, height, samples) {
                    self.pixels = Pixels::F32(Plane::Tiled(plane));
                }
            }
            Pixels::U16(Plane::Tiled(_)) | Pixels::F32(Plane::Tiled(_)) => {}
        }
    }

    /// [`Self::tile_unseeded`], except that a bitwise-uniform plane becomes
    /// ONE filled plane (`uniform_tiles`) and the adopted buffer is dropped.
    /// Nothing cached; identity is unchanged and no job is polled.
    fn tile_decoded(&mut self) {
        let (width, height) = (self.width, self.height);
        match &self.pixels {
            Pixels::U16(Plane::Contiguous(samples)) => {
                if let Some(plane) = uniform_tiles(width, height, samples) {
                    self.pixels = Pixels::U16(Plane::Tiled(plane));
                    return;
                }
            }
            Pixels::F32(Plane::Contiguous(samples)) => {
                if let Some(plane) = uniform_tiles(width, height, samples) {
                    self.pixels = Pixels::F32(Plane::Tiled(plane));
                    return;
                }
            }
            Pixels::U16(Plane::Tiled(_)) | Pixels::F32(Plane::Tiled(_)) => {}
        }
        self.tile_unseeded();
    }

    /// Contiguous → Tiled over `previous`'s tiles (`Plane::rebuilt_tiles`:
    /// same storage, tiled, same dimensions), nothing cached; anything else
    /// is [`Self::tile_unseeded`]. Since 13B-3d-3a no rebuild seeds the view
    /// cache: every `rebuilt_*` output is installed as a layer whose readers
    /// take tiles, and a whole reader pays one materialization on its first
    /// read. Identity is unchanged and no job is polled.
    fn rebuild_over(&mut self, previous: &Self) {
        let (width, height) = (self.width, self.height);
        match (&self.pixels, &previous.pixels) {
            (Pixels::U16(Plane::Contiguous(samples)), Pixels::U16(old)) => {
                if let Some(plane) = old.rebuilt_tiles(width, height, samples) {
                    self.pixels = Pixels::U16(Plane::Tiled(plane));
                    return;
                }
            }
            (Pixels::F32(Plane::Contiguous(samples)), Pixels::F32(old)) => {
                if let Some(plane) = old.rebuilt_tiles(width, height, samples) {
                    self.pixels = Pixels::F32(Plane::Tiled(plane));
                    return;
                }
            }
            _ => {}
        }
        self.tile_unseeded();
    }

    fn tiled_by(&mut self, tiling: Tiling<'_>) {
        match tiling {
            Tiling::Seeded => self.tile(),
            Tiling::Unseeded => self.tile_unseeded(),
            Tiling::Decoded => self.tile_decoded(),
            Tiling::Rebuilt(previous) => self.rebuild_over(previous),
            Tiling::Kept => {}
        }
    }

    /// Takes the caller's existing allocation; scratch excludes that input.
    pub fn from_rgba16(
        width: u32,
        height: u32,
        pixels: Vec<[u16; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        Self::adopted_rgba16(width, height, pixels, profile, control, Tiling::Seeded)
    }

    /// [`Self::from_rgba16`] with the seed dropped: the same statements,
    /// refusals and checkpoints; the plane tiles and nothing is cached. For a
    /// plane nothing reads whole right away (the promotion, the placeholders,
    /// the Copy / Cut payload and, since 13B-3d-3a, the layer producers —
    /// the artwork render, the warp output, Rasterize, the paste
    /// conversion and the retouch new layer).
    pub fn from_rgba16_unseeded(
        width: u32,
        height: u32,
        pixels: Vec<[u16; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        Self::adopted_rgba16(width, height, pixels, profile, control, Tiling::Unseeded)
    }

    /// [`Self::from_rgba16_unseeded`] for a DECODED plane (13B-3e-4b): the
    /// same statements, refusals and checkpoints, a fresh identity, no seed,
    /// and a bitwise-uniform plane (a reopened empty layer, a pixel-less PSD
    /// record) is ONE filled plane. Every reader-side u16 decoder builds
    /// through it: the typed `.rz` layer / master PNG, the WARP original,
    /// the 16-bit PNG / TIFF open, the 16-bit PSD layer and composite and the
    /// RZCB / RGBA8 clipboard payloads.
    pub(crate) fn from_rgba16_decoded(
        width: u32,
        height: u32,
        pixels: Vec<[u16; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        Self::adopted_rgba16(width, height, pixels, profile, control, Tiling::Decoded)
    }

    /// The image holding `pixels`, sharing every tile of `self` that
    /// `pixels` leaves bitwise unchanged (u16, tiled, same dimensions), else
    /// [`Self::from_rgba16_unseeded`]'s tiles; nothing seeds the view cache
    /// either way (13B-3d-3a). The same length check and admission as
    /// `from_rgba16`; no job. ALWAYS a fresh identity, even when nothing
    /// changed.
    pub fn rebuilt_rgba16(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[u16; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        Self::adopted_rgba16(width, height, pixels, profile, None, Tiling::Rebuilt(self))
    }

    fn adopted_rgba16(
        width: u32,
        height: u32,
        pixels: Vec<[u16; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
        tiling: Tiling<'_>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision pixel buffer length does not match dimensions".into());
        }
        let mut out = Self::assembled(
            width,
            height,
            profile,
            Pixels::U16(Plane::Contiguous(Arc::new(pixels))),
        );
        admit(
            out.retained_bytes(),
            BOOKKEEPING_BYTES,
            0,
            process_working_limit(),
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        out.tiled_by(tiling);
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// All four components must be finite in [0,1]; no implicit HDR clamp.
    pub fn from_rgba32(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision pixel buffer length does not match dimensions".into());
        }
        let mut out = Self::assembled(
            width,
            height,
            profile,
            Pixels::F32(Plane::Contiguous(Arc::new(pixels))),
        );
        admit(
            out.retained_bytes(),
            BOOKKEEPING_BYTES,
            0,
            process_working_limit(),
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let Pixels::F32(Plane::Contiguous(samples)) = &out.pixels else {
            unreachable!("a constructor's plane is contiguous until tile()");
        };
        for (i, pixel) in samples.iter().enumerate() {
            job.every(i)?;
            if pixel
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(
                    "precision float components must be finite and normalized to [0,1]".into(),
                );
            }
        }
        out.tile_unseeded();
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The EXTENDED-RANGE twin of [`Self::from_rgba32`] (Phase 11F): RGB may
    /// be any finite value with `|v| ≤ FLOAT_RANGE_MAX` — above white and
    /// below zero alike — and alpha must still be coverage in [0, 1]. There
    /// is no repair here: a caller sanitises at IMPORT (`float_document::
    /// sanitize`) and everything downstream refuses.
    pub fn from_rgba32_extended(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        Self::adopted_rgba32_extended(width, height, pixels, profile, control, Tiling::Unseeded)
    }

    /// `from_rgba32_extended`'s statements and refusals with the plane left
    /// Contiguous (`Tiling::Kept`), as a tile-store refusal leaves it. Test
    /// seam for the Contiguous arms.
    #[doc(hidden)]
    pub fn contiguous_rgba32_extended(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        Self::adopted_rgba32_extended(width, height, pixels, profile, None, Tiling::Kept)
    }

    /// The f32 twin of [`Self::rebuilt_rgba16`] over
    /// [`Self::from_rgba32_extended`]'s validation (every sample
    /// `extended_pixel`, else its sentence); no seed, a fresh identity.
    pub fn rebuilt_rgba32_extended(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
    ) -> Result<Self, String> {
        Self::adopted_rgba32_extended(width, height, pixels, profile, None, Tiling::Rebuilt(self))
    }

    /// The f32 twin of [`Self::from_rgba16_decoded`] over
    /// [`Self::from_rgba32_extended`]'s validation: the f32 `.rz` raster, the
    /// 32-bit float TIFF open and the f32 RZCB payload.
    pub(crate) fn from_rgba32_extended_decoded(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        Self::adopted_rgba32_extended(width, height, pixels, profile, control, Tiling::Decoded)
    }

    fn adopted_rgba32_extended(
        width: u32,
        height: u32,
        pixels: Vec<[f32; 4]>,
        profile: Arc<IccProfile>,
        control: Option<&DrJobControl>,
        tiling: Tiling<'_>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision pixel buffer length does not match dimensions".into());
        }
        let mut out = Self::assembled(
            width,
            height,
            profile,
            Pixels::F32(Plane::Contiguous(Arc::new(pixels))),
        );
        admit(
            out.retained_bytes(),
            BOOKKEEPING_BYTES,
            0,
            process_working_limit(),
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let Pixels::F32(Plane::Contiguous(samples)) = &out.pixels else {
            unreachable!("a constructor's plane is contiguous until tile()");
        };
        for (i, pixel) in samples.iter().enumerate() {
            job.every(i)?;
            if !extended_pixel(*pixel) {
                return Err(EXTENDED.into());
            }
        }
        out.tiled_by(tiling);
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The image with the rect overwritten by `samples` (row-major, stride
    /// `w`): `TilePlane::with_rect` semantics — `Err("precision image
    /// storage is not RGBA16")` on f32 storage, `Err` on a sample-count
    /// mismatch, a rect leaving the plane or an allocation failure,
    /// `Ok(None)` for a zero area or no bitwise change. `Some` is a fresh
    /// identity with the same profile sharing every untouched tile; NO view
    /// is seeded. A contiguous image is compared, then written straight into
    /// tiles at every size, above `CONTIGUOUS_CACHE_BYTES` too (no whole
    /// contiguous copy).
    pub fn with_rect_rgba16(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[[u16; 4]],
    ) -> Result<Option<Self>, String> {
        let Pixels::U16(plane) = &self.pixels else {
            return Err("precision image storage is not RGBA16".into());
        };
        Ok(plane
            .with_rect(self.dimensions(), (x, y, w, h), samples)?
            .map(|next| self.sharing(Pixels::U16(next))))
    }

    /// The f32 twin of [`Self::with_rect_rgba16`]; additionally
    /// `Err(from_rgba32_extended's sentence)` when any of `samples` fails
    /// `extended_pixel`.
    pub fn with_rect_rgba32(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[[f32; 4]],
    ) -> Result<Option<Self>, String> {
        let Pixels::F32(plane) = &self.pixels else {
            return Err("precision image storage is not RGBA32 float".into());
        };
        if !samples.iter().all(|pixel| extended_pixel(*pixel)) {
            return Err(EXTENDED.into());
        }
        Ok(plane
            .with_rect(self.dimensions(), (x, y, w, h), samples)?
            .map(|next| self.sharing(Pixels::F32(next))))
    }

    /// The pixels of the tiles `with_rect_rgba16` / `with_rect_rgba32` can
    /// allocate writing this rect: tiled, the rect rounded out to `TILE_EDGE`
    /// and clipped to the plane (a write copies every changed tile whole);
    /// contiguous, `width·height` (the write builds a whole tiled plane); 0
    /// for a zero area or a rect leaving the plane. The one statement of that
    /// bound, for a plan that charges the write.
    pub fn rect_write_pixels(&self, x: u32, y: u32, w: u32, h: u32) -> u64 {
        let (width, height) = self.dimensions();
        let (x1, y1) = (u64::from(x) + u64::from(w), u64::from(y) + u64::from(h));
        if w == 0 || h == 0 || x1 > u64::from(width) || y1 > u64::from(height) {
            return 0;
        }
        let tiled = match &self.pixels {
            Pixels::U16(plane) => plane.is_tiled(),
            Pixels::F32(plane) => plane.is_tiled(),
        };
        if !tiled {
            return u64::from(width) * u64::from(height);
        }
        let edge = u64::from(TILE_EDGE);
        let (left, top) = (u64::from(x) / edge * edge, u64::from(y) / edge * edge);
        let right = (x1.div_ceil(edge) * edge).min(u64::from(width));
        let bottom = (y1.div_ceil(edge) * edge).min(u64::from(height));
        (right - left) * (bottom - top)
    }

    /// The rect's u16 samples, row-major with stride `w`
    /// (`TilePlane::read_rect` semantics); `None` for f32 storage, a zero
    /// area, a rect leaving the plane or an allocation failure. Binds no
    /// view.
    pub fn read_rect_rgba16(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<[u16; 4]>> {
        match &self.pixels {
            Pixels::U16(plane) => plane.read_rect(self.dimensions(), (x, y, w, h)),
            Pixels::F32(_) => None,
        }
    }

    /// The f32 twin of [`Self::read_rect_rgba16`].
    pub fn read_rect_rgba32(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<[f32; 4]>> {
        match &self.pixels {
            Pixels::F32(plane) => plane.read_rect(self.dimensions(), (x, y, w, h)),
            Pixels::U16(_) => None,
        }
    }

    /// The same samples, profile and representation under a fresh identity
    /// token; `pinned` starts empty. A render that places its source
    /// unchanged answers this, so the layer shares every tile of the source.
    pub(crate) fn reidentified(&self) -> Self {
        self.sharing(self.pixels.clone())
    }

    /// A fresh identity over `pixels` with this image's dimensions and
    /// profile.
    fn sharing(&self, pixels: Pixels) -> Self {
        Self::assembled(self.width, self.height, self.profile.clone(), pixels)
    }

    /// [`Self::from_float_samples_unseeded`] over `self`'s tiles (`Tiling::Rebuilt`,
    /// [`Self::rebuilt_rgba16`]'s rule: every bitwise-unchanged tile shared,
    /// nothing seeded, always a fresh identity).
    pub(crate) fn rebuilt_float_samples(
        &self,
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        storage: PrecisionStorage,
        samples: impl Iterator<Item = [f32; 4]>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let tiling = Tiling::Rebuilt(self);
        Self::float_samples(width, height, profile, storage, samples, job, tiling)
    }

    /// A render's samples, validated (finite) and clamped to [0, 1] at
    /// `storage`; the plane tiles and nothing is cached (13B-3c-2b; its
    /// seeded twin went in 13B-3e-4b).
    pub(crate) fn from_float_samples_unseeded(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        storage: PrecisionStorage,
        samples: impl Iterator<Item = [f32; 4]>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Self::float_samples(
            width,
            height,
            profile,
            storage,
            samples,
            job,
            Tiling::Unseeded,
        )
    }

    fn float_samples(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        storage: PrecisionStorage,
        samples: impl Iterator<Item = [f32; 4]>,
        job: JobContext<'_>,
        tiling: Tiling<'_>,
    ) -> Result<Self, String> {
        let count = pixel_count(width, height)?;
        let pixels = match storage {
            PrecisionStorage::Rgba16Unorm => {
                let mut out = reserved_vec(count)?;
                for (i, pixel) in samples.enumerate() {
                    job.every(i)?;
                    if i >= count || pixel.iter().any(|v| !v.is_finite()) {
                        return Err(
                            "invalid precision sample count or nonfinite render output".into()
                        );
                    }
                    out.push(pixel.map(unit_to_u16));
                }
                if out.len() != count {
                    return Err("precision sample count differs from dimensions".into());
                }
                Pixels::U16(Plane::Contiguous(Arc::new(out)))
            }
            PrecisionStorage::Rgba32Float => {
                let mut out = reserved_vec(count)?;
                for (i, pixel) in samples.enumerate() {
                    job.every(i)?;
                    if i >= count || pixel.iter().any(|v| !v.is_finite()) {
                        return Err(
                            "invalid precision sample count or nonfinite render output".into()
                        );
                    }
                    out.push(pixel.map(|v| v.clamp(0.0, 1.0)));
                }
                if out.len() != count {
                    return Err("precision sample count differs from dimensions".into());
                }
                Pixels::F32(Plane::Contiguous(Arc::new(out)))
            }
        };
        let mut out = Self::assembled(width, height, profile, pixels);
        out.tiled_by(tiling);
        job.check()?;
        Ok(out)
    }

    /// `extended` is the 32-bit FLOAT mode (Phase 11F): the result keeps
    /// every above-white and negative component exactly and is REFUSED — not
    /// repaired — when it is not finite or leaves the stored range. The
    /// normalized path is unchanged: finite, then clamped to [0, 1]. The
    /// plane tiles and nothing is cached (13B-3c-2b; its seeded twin went
    /// in 13B-3e-4b).
    pub(crate) fn from_rendered_float_unseeded(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        pixels: Vec<[f32; 4]>,
        extended: bool,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Self::rendered_float(
            width,
            height,
            profile,
            pixels,
            extended,
            job,
            Tiling::Unseeded,
        )
    }

    fn rendered_float(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        mut pixels: Vec<[f32; 4]>,
        extended: bool,
        job: JobContext<'_>,
        tiling: Tiling<'_>,
    ) -> Result<Self, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision sample count differs from dimensions".into());
        }
        for (i, pixel) in pixels.iter_mut().enumerate() {
            job.every(i)?;
            if pixel.iter().any(|v| !v.is_finite()) {
                return Err("nonfinite precision render output".into());
            }
            if extended {
                if !extended_pixel(*pixel) {
                    return Err("32-bit float render output leaves the stored ±65504 range".into());
                }
                continue;
            }
            *pixel = pixel.map(|v| v.clamp(0.0, 1.0));
        }
        let mut out = Self::assembled(
            width,
            height,
            profile,
            Pixels::F32(Plane::Contiguous(Arc::new(pixels))),
        );
        out.tiled_by(tiling);
        job.check()?;
        Ok(out)
    }
}
