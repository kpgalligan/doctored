//! `GrayRaster` — the ONE accessor for a shared u8 coverage plane (13B-4): a
//! byte layer mask (`Layer.mask`), a byte channel (`Channel.data`), a live
//! vector mask's raster part and baked plane, a WARP binding's mask planes and
//! the PATH table's byte planes. `LayerRaster`'s contract, one sample wide.
//!
//! **Representation** is private: `Tiled(TilePlane<u8>)`, or
//! `Contiguous(Arc<GrayImage>)` when the tile store refuses the image (zero
//! area, above `MAX_PIXELS`, an allocation failure) and when a reader ADOPTS a
//! plane it built for one projection ([`GrayRaster::shared`]: a group's
//! effective-mask window, the aligned and derived mask caches). Every
//! constructor is infallible, so a refusal is never a silent loss.
//!
//! **Writers.** [`GrayRaster::rebuilt`] (every whole-plane writer: invert,
//! the scoped adjust and filter, `set_channel_plane`, the selection-to-plane
//! writers) shares each bitwise-unchanged tile with the raster it replaces,
//! and [`GrayRaster::with_rect`] allocates only the tiles its rect changes, so
//! a 300×300 mask or channel dab on a 24 MP plane retains at most 4 new tiles
//! instead of a whole copy (the two dab writers, `paint_mask` and
//! `painting_channel`, read only the overlay's rect through `read_rect`).
//!
//! **Readers read in place.** There is NO view cache for gray planes and no
//! `Deref<Target = GrayImage>`: the compositor reads a mask one row span at a
//! time through [`GrayRaster::row`] (a tile's own run when the span lies in
//! one tile column, else one row copied into the caller's scratch), the
//! per-sample readers through [`GrayRaster::sample_at`], and the persistence
//! writers through [`GrayRaster::runs`] or one transient
//! [`GrayRaster::to_image`] copy. So a projection of a masked layer leaves no
//! whole-plane copy behind, and a dab is never re-paid as a whole-plane copy
//! on the next read.
//!
//! **Identity** is a private `Arc<()>` token minted by `new` / `shared` /
//! `rebuilt` / `with_rect` (`Some`) / `reidentified` and shared by clones,
//! never the bytes or a tile: `ptr_eq`, `key()` (the mask caches, the style
//! cache and the content-box memo hold a [`GrayRasterKey`]) and
//! `storage_identity()` (`dr_doc_layer_mask_storage_identity`). A whole-plane
//! writer answers a FRESH identity even when every tile is shared.
//!
//! **Equality** by name only: `same_bytes` (raster to raster) and
//! `equals_image` (raster to an owned image). No `PartialEq`.
//!
//! **Accounting.** `record_resources` records the index as `tile_structure`
//! and each tile under the caller's category (`layer_masks`,
//! `saved_channels`, `warp_masks`), so a tile shared by several undo states
//! counts once; a contiguous raster keeps the two records it had as an
//! `Arc<GrayImage>`.

use std::sync::{Arc, Weak};

use image::GrayImage;

use crate::job_control::JobContext;
use crate::resources::Inventory;
use crate::tile_store::{RowSegments, TilePlane};

/// The allocation-failure sentence of every gray-raster copy.
pub(crate) const ALLOCATION: &str = "could not allocate coverage plane storage";

/// A shared, immutable u8 coverage plane.
#[derive(Clone)]
pub struct GrayRaster {
    repr: Repr,
    identity: Arc<()>,
}

#[derive(Clone)]
enum Repr {
    Tiled(TilePlane<u8>),
    Contiguous(Arc<GrayImage>),
}

/// The exact `w·h` samples of `image` (a `GrayImage` buffer may be longer).
fn plane_samples(image: &GrayImage) -> &[u8] {
    let len = (image.width() as usize * image.height() as usize).min(image.as_raw().len());
    &image.as_raw()[..len]
}

impl From<GrayImage> for GrayRaster {
    fn from(image: GrayImage) -> Self {
        Self::new(image)
    }
}

impl GrayRaster {
    /// Tiles `image` under a fresh identity; contiguous when the tile store
    /// refuses it. A bitwise-uniform image (a Reveal All or Hide All mask, a
    /// uniform channel) is ONE filled plane, at most four tile allocations
    /// (`TilePlane::uniform_from`), so a history state that adds one retains
    /// a few tiles rather than a whole plane of identical ones (13B-5).
    pub fn new(image: GrayImage) -> Self {
        let (width, height) = image.dimensions();
        let samples = plane_samples(&image);
        if let Some(plane) = TilePlane::uniform_from(width, height, samples) {
            return Self::tiled(plane);
        }
        match TilePlane::from_contiguous(width, height, samples, None) {
            Ok(plane) => Self::tiled(plane),
            Err(_) => Self::contiguous(Arc::new(image)),
        }
    }

    /// Adopts `image` without copying, under a fresh identity: for a plane
    /// built for one read (a group's effective-mask window, the mask caches'
    /// aligned and derived planes). Its first write tiles it.
    pub fn shared(image: Arc<GrayImage>) -> Self {
        Self::contiguous(image)
    }

    /// The raster holding `image`, sharing every tile of `self` that `image`
    /// leaves bitwise unchanged (same dimensions, tiled `self`), else
    /// [`Self::new`]. Always a fresh identity, even when nothing changed.
    pub fn rebuilt(&self, image: GrayImage) -> Self {
        let Repr::Tiled(plane) = &self.repr else {
            return Self::new(image);
        };
        if plane.dimensions() != image.dimensions() {
            return Self::new(image);
        }
        match plane.rebuilt_from(plane_samples(&image), None) {
            Ok(Some(next)) => Self::tiled(next),
            Ok(None) => self.reidentified(),
            Err(_) => Self::new(image),
        }
    }

    /// The raster with the rect overwritten by `samples` (row-major, stride
    /// `w`): `TilePlane::with_rect` semantics — `Ok(None)` when no sample
    /// changed (or the rect is empty), `Err` on a sample-count mismatch, a
    /// rect leaving the plane or an allocation failure. `Some` is a fresh
    /// identity sharing every untouched tile. A contiguous raster answers a
    /// tiled copy.
    pub fn with_rect(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[u8],
    ) -> Result<Option<Self>, String> {
        self.with_rect_with_job(x, y, w, h, samples, JobContext::NONE)
    }

    /// [`Self::with_rect`] polling `job` once per visited tile (a contiguous
    /// raster: once per rect row of the compare, then its copy's checks).
    pub(crate) fn with_rect_with_job(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[u8],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let image = match &self.repr {
            Repr::Tiled(plane) => {
                return Ok(plane
                    .with_rect_with_job(x, y, w, h, samples, job)?
                    .map(Self::tiled));
            }
            Repr::Contiguous(image) => image,
        };
        if samples.len() as u64 != u64::from(w) * u64::from(h) {
            return Err("tile plane sample count does not match dimensions".into());
        }
        if w == 0 || h == 0 {
            return Ok(None);
        }
        let (width, height) = image.dimensions();
        if u64::from(x) + u64::from(w) > u64::from(width)
            || u64::from(y) + u64::from(h) > u64::from(height)
        {
            return Err("tile rect leaves the plane".into());
        }
        let (stride, span) = (width as usize, w as usize);
        let at = |row: usize| (y as usize + row) * stride + x as usize;
        let raw = plane_samples(image);
        let mut same = true;
        for (row, line) in samples.chunks_exact(span).enumerate() {
            job.check()?;
            if raw[at(row)..at(row) + span] != *line {
                same = false;
                break;
            }
        }
        if same {
            return Ok(None);
        }
        let mut copy = self.to_image_with_job(job)?;
        let target: &mut [u8] = &mut copy;
        for (row, line) in samples.chunks_exact(span).enumerate() {
            target[at(row)..at(row) + span].copy_from_slice(line);
        }
        Ok(Some(Self::new(copy)))
    }

    /// The same samples in the same representation under a fresh identity
    /// (a writer whose edit changed no sample still answers a new plane).
    pub(crate) fn reidentified(&self) -> Self {
        Self {
            repr: self.repr.clone(),
            identity: Arc::new(()),
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        match &self.repr {
            Repr::Tiled(plane) => plane.dimensions(),
            Repr::Contiguous(image) => image.dimensions(),
        }
    }

    pub fn width(&self) -> u32 {
        self.dimensions().0
    }

    pub fn height(&self) -> u32 {
        self.dimensions().1
    }

    /// `w·h`, the payload in bytes, without a copy.
    pub fn byte_len(&self) -> u64 {
        let (width, height) = self.dimensions();
        u64::from(width) * u64::from(height)
    }

    /// The sample at `(x, y)`; `None` outside the plane. No allocation.
    pub fn sample_at(&self, x: u32, y: u32) -> Option<u8> {
        match &self.repr {
            Repr::Tiled(plane) => plane.sample_at(x, y),
            Repr::Contiguous(image) => {
                let (width, height) = image.dimensions();
                if x >= width || y >= height {
                    return None;
                }
                Some(plane_samples(image)[y as usize * width as usize + x as usize])
            }
        }
    }

    /// Row `y`, columns `x0..x1` (exclusive), as contiguous runs `(x,
    /// samples)`, left to right and adjoining: `TilePlane::row_segments`'s
    /// rules on both representations (empty for `y >= height` or `x0 >= x1`;
    /// `x1` clamped to `width`; one run for a contiguous plane). No
    /// allocation.
    pub fn row_segments(&self, y: u32, x0: u32, x1: u32) -> GrayRowSegments<'_> {
        let inner = match &self.repr {
            Repr::Tiled(plane) => Segments::Tiled(plane.row_segments(y, x0, x1)),
            Repr::Contiguous(image) => {
                let (width, height) = image.dimensions();
                let x1 = x1.min(width);
                let run = (y < height && x0 < x1).then(|| {
                    let row = y as usize * width as usize;
                    (
                        x0,
                        &plane_samples(image)[row + x0 as usize..row + x1 as usize],
                    )
                });
                Segments::Contiguous(run)
            }
        };
        GrayRowSegments { inner }
    }

    /// Row `y`, columns `x0..x1` (`x1` clamped to `width`) as ONE slice: the
    /// tile's own run when the span lies inside one tile column (or the plane
    /// is contiguous), else the runs copied into `scratch`. Empty for `y >=
    /// height` or `x0 >= x1`. The compositor's per-row mask read: it never
    /// copies more than one row span.
    pub(crate) fn row<'a>(
        &'a self,
        y: u32,
        x0: u32,
        x1: u32,
        scratch: &'a mut Vec<u8>,
    ) -> &'a [u8] {
        let mut runs = self.row_segments(y, x0, x1);
        let Some((_, first)) = runs.next() else {
            return &[];
        };
        let Some((_, second)) = runs.next() else {
            return first;
        };
        scratch.clear();
        scratch.extend_from_slice(first);
        scratch.extend_from_slice(second);
        for (_, run) in runs {
            scratch.extend_from_slice(run);
        }
        scratch
    }

    /// Every row's runs in plane order (row-major, left to right). No
    /// allocation.
    pub fn runs(&self) -> impl Iterator<Item = &[u8]> + '_ {
        let (width, height) = self.dimensions();
        (0..height).flat_map(move |y| self.row_segments(y, 0, width).map(|(_, run)| run))
    }

    /// `tile_store::TilePlane::read_rect` semantics: row-major samples;
    /// zero-area → `Some(vec![])`; a rect that leaves the plane, or a copy
    /// that cannot be allocated → `None`. Bounds are checked in u64.
    pub fn read_rect(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<u8>> {
        let (width, height) = self.dimensions();
        if u64::from(x) + u64::from(w) > u64::from(width)
            || u64::from(y) + u64::from(h) > u64::from(height)
        {
            return None;
        }
        if w == 0 || h == 0 {
            return Some(Vec::new());
        }
        let image = match &self.repr {
            Repr::Tiled(plane) => return plane.read_rect(x, y, w, h),
            Repr::Contiguous(image) => image,
        };
        let mut out = Vec::new();
        out.try_reserve_exact(w as usize * h as usize).ok()?;
        let raw = plane_samples(image);
        for row in y as usize..(y + h) as usize {
            let start = row * width as usize + x as usize;
            out.extend_from_slice(&raw[start..start + w as usize]);
        }
        Some(out)
    }

    /// One owned row-major copy of the plane, admitted through
    /// `precision_image::reserved_vec`. Err: [`ALLOCATION`].
    pub fn to_image(&self) -> Result<GrayImage, String> {
        self.to_image_with_job(JobContext::NONE)
    }

    /// [`Self::to_image`] polling `job`: once per tile (tiled), once before
    /// the copy (contiguous). Err: [`ALLOCATION`] for an allocation failure,
    /// the control's own sentence for a cancel.
    pub(crate) fn to_image_with_job(&self, job: JobContext<'_>) -> Result<GrayImage, String> {
        let (width, height) = self.dimensions();
        let raw = match &self.repr {
            Repr::Tiled(plane) => plane.to_contiguous_with_job(job).map_err(|e| {
                if job.cancelled() {
                    e
                } else {
                    ALLOCATION.to_string()
                }
            })?,
            Repr::Contiguous(image) => {
                job.check()?;
                let raw = plane_samples(image);
                let mut out = crate::precision_image::reserved_vec::<u8>(raw.len())
                    .map_err(|_| ALLOCATION.to_string())?;
                out.extend_from_slice(raw);
                out
            }
        };
        GrayImage::from_raw(width, height, raw).ok_or_else(|| ALLOCATION.to_string())
    }

    /// Bytes equality: `false` when dimensions differ; tiles through
    /// `TilePlane::same_bits`, a mixed pair through the tiled side's
    /// `equals_contiguous`, two contiguous planes by `Arc::ptr_eq` or their
    /// `w·h` samples. Symmetric; no allocation.
    pub fn same_bytes(&self, other: &Self) -> bool {
        if self.dimensions() != other.dimensions() {
            return false;
        }
        match (&self.repr, &other.repr) {
            (Repr::Tiled(a), Repr::Tiled(b)) => a.same_bits(b),
            (Repr::Contiguous(a), Repr::Contiguous(b)) => {
                Arc::ptr_eq(a, b) || plane_samples(a) == plane_samples(b)
            }
            (Repr::Tiled(_), Repr::Contiguous(image)) => self.equals_image(image),
            (Repr::Contiguous(image), Repr::Tiled(_)) => other.equals_image(image),
        }
    }

    /// Bytes equality with an owned image of the same dimensions (its first
    /// `w·h` bytes). No allocation.
    pub fn equals_image(&self, image: &GrayImage) -> bool {
        if image.dimensions() != self.dimensions() {
            return false;
        }
        let raw = image.as_raw();
        let len = image.width() as usize * image.height() as usize;
        if raw.len() < len {
            return false;
        }
        match &self.repr {
            Repr::Tiled(plane) => plane.equals_contiguous(&raw[..len]),
            Repr::Contiguous(own) => plane_samples(own) == &raw[..len],
        }
    }

    /// The row-major samples as one owned `Vec` (`w·h` bytes). Test oracle:
    /// a library reader uses `runs`, `row`, `sample_at` or `to_image`. Panics
    /// when the copy cannot be allocated.
    #[doc(hidden)]
    pub fn samples(&self) -> Vec<u8> {
        self.to_image().expect(ALLOCATION).into_raw()
    }

    /// `image::GrayImage::get_pixel`'s contract (panics outside the plane).
    /// Test oracle: a library reader uses `sample_at`.
    #[doc(hidden)]
    pub fn get_pixel(&self, x: u32, y: u32) -> image::Luma<u8> {
        let (width, height) = self.dimensions();
        image::Luma([self
            .sample_at(x, y)
            .unwrap_or_else(|| panic!("pixel ({x}, {y}) outside a {width}x{height} plane"))])
    }

    /// Tiles `self` and `other` share by pointer at the same index; 0 unless
    /// both are tiled with equal dimensions. Test oracle.
    pub fn shared_tile_count(&self, other: &Self) -> usize {
        match (&self.repr, &other.repr) {
            (Repr::Tiled(a), Repr::Tiled(b)) => a.shared_tile_count(b),
            _ => 0,
        }
    }

    /// Identity (the token, `Arc::ptr_eq`).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.identity, &b.identity)
    }

    /// The identity key the mask caches, the style cache and the content-box
    /// memo hold.
    pub(crate) fn key(&self) -> GrayRasterKey {
        GrayRasterKey {
            token: Arc::downgrade(&self.identity),
        }
    }

    /// The address of the identity token as an opaque storage identity
    /// (`dr_doc_layer_mask_storage_identity`): equal for two rasters exactly
    /// when [`Self::ptr_eq`] holds, for as long as either is alive.
    pub(crate) fn storage_identity(&self) -> u64 {
        Arc::as_ptr(&self.identity) as usize as u64
    }

    /// Tiled: the index as `tile_structure` and every tile under `category`
    /// (a tile shared by several planes counts once). Contiguous: the image
    /// structure (`shared_buffer_structure`) and its samples under
    /// `category`, the records an `Arc<GrayImage>` plane had.
    pub(crate) fn record_resources(
        &self,
        inventory: &mut Inventory,
        category: &'static str,
        cache: bool,
    ) {
        match &self.repr {
            Repr::Tiled(plane) => plane.record_resources(inventory, category, cache),
            Repr::Contiguous(image) => {
                inventory.object(&**image, "shared_buffer_structure", cache);
                inventory.vec(image.as_raw(), category, cache);
            }
        }
    }

    /// A mask cache's plane (`MaskProperties::cached_planes`): a contiguous
    /// plane as one `mask_cache_structure` object and its samples under
    /// `mask_cache_planes`, the records it had as an `Arc<GrayImage>`; a
    /// tiled one through [`Self::record_resources`] under `mask_cache_planes`.
    /// Both as cache.
    pub(crate) fn record_cache_resources(&self, inventory: &mut Inventory) {
        match &self.repr {
            Repr::Tiled(plane) => plane.record_resources(inventory, "mask_cache_planes", true),
            Repr::Contiguous(image) => {
                inventory.object(&**image, "mask_cache_structure", true);
                inventory.vec(image.as_raw(), "mask_cache_planes", true);
            }
        }
    }

    /// The tiled plane, `None` for the contiguous form: the tile pool's write
    /// (13C) keys these tiles and writes a contiguous plane's bytes as a blob.
    pub(crate) fn tile_plane(&self) -> Option<&TilePlane<u8>> {
        match &self.repr {
            Repr::Tiled(plane) => Some(plane),
            Repr::Contiguous(_) => None,
        }
    }

    fn tiled(plane: TilePlane<u8>) -> Self {
        Self {
            repr: Repr::Tiled(plane),
            identity: Arc::new(()),
        }
    }

    fn contiguous(image: Arc<GrayImage>) -> Self {
        Self {
            repr: Repr::Contiguous(image),
            identity: Arc::new(()),
        }
    }
}

/// The runs of one row span (see [`GrayRaster::row_segments`]).
pub struct GrayRowSegments<'a> {
    inner: Segments<'a>,
}

enum Segments<'a> {
    Tiled(RowSegments<'a, u8>),
    Contiguous(Option<(u32, &'a [u8])>),
}

impl<'a> Iterator for GrayRowSegments<'a> {
    type Item = (u32, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            Segments::Tiled(runs) => runs.next(),
            Segments::Contiguous(run) => run.take(),
        }
    }
}

/// A non-owning identity handle onto a [`GrayRaster`].
#[derive(Clone)]
pub(crate) struct GrayRasterKey {
    token: Weak<()>,
}

impl GrayRasterKey {
    /// Same identity (`std::ptr::eq` on the weak pointers).
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::ptr::eq(self.token.as_ptr(), other.token.as_ptr())
    }

    /// Whether this key names `raster`.
    pub(crate) fn names(&self, raster: &GrayRaster) -> bool {
        std::ptr::eq(self.token.as_ptr(), Arc::as_ptr(&raster.identity))
    }
}
