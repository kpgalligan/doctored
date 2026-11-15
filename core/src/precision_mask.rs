//! Immutable native u16 coverage for typed masks and channels. Raw coverage is
//! independent of raster dimensions. Feather shares the legacy blur schedule;
//! effective density/feather is f32 and never overwrites the raw source.
//!
//! Effective mask inspection ignores the enabled flag; rendering bypasses disabled masks. Float
//! feather shares `plane_blur.rs` with legacy byte masks/styles; no retained typed blur cache.
//!
//! Since 13B-4 a [`PrecisionPlane`] is a `tile_store::TilePlane<u16>` of 256×256 tiles behind a
//! private identity token (the byte `GrayRaster`'s contract at 16 bits): `rebuilt` shares every
//! bitwise-unchanged tile, so a mask or channel dab retains the tiles it changes; readers walk
//! `runs` / `row_segments` / `sample_at` in place or bind one transient `to_contiguous[_with_job]`
//! copy; identity is `ptr_eq` / `key` / `storage_identity`, never a sample address. `samples()`
//! is the test oracles' owned copy and has no library caller.
use crate::doc_mask::{density_coverage, MaskProperties};
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::{
    admit, pixel_count, process_working_limit, reserved_vec, BOOKKEEPING_BYTES,
};
use crate::resources::Inventory;
use crate::tile_store::{RowSegments, TilePlane};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Weak,
};

/// A typed coverage plane: immutable u16 samples on shared tiles.
#[derive(Clone)]
pub struct PrecisionPlane {
    tiles: TilePlane<u16>,
    identity: Arc<()>,
}

/// A non-owning identity handle onto a [`PrecisionPlane`] (the typed
/// content-box memo and style cache keys).
#[derive(Clone)]
pub(crate) struct PrecisionPlaneKey {
    token: Weak<()>,
}

impl PrecisionPlaneKey {
    /// Whether this key names `plane`.
    pub(crate) fn names(&self, plane: &PrecisionPlane) -> bool {
        std::ptr::eq(self.token.as_ptr(), Arc::as_ptr(&plane.identity))
    }

    /// Same identity.
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::ptr::eq(self.token.as_ptr(), other.token.as_ptr())
    }

    /// The plane this key named has been dropped.
    pub(crate) fn is_dead(&self) -> bool {
        self.token.strong_count() == 0
    }
}

impl PrecisionPlane {
    /// Tiles an owned native-endian plane (no byte promotion).
    pub fn from_u16(
        width: u32,
        height: u32,
        pixels: Vec<u16>,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = Self::adopt(width, height, pixels)?;
        admit(
            out.storage_bytes(),
            BOOKKEEPING_BYTES,
            0,
            process_working_limit(),
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        job.report(1.0, 5)?;
        Ok(out)
    }
    /// Tiles `pixels` (`w·h` samples) under a fresh identity; the Vec is
    /// dropped once its samples are in the tiles.
    pub(crate) fn adopt(width: u32, height: u32, pixels: Vec<u16>) -> Result<Self, String> {
        Self::copy(width, height, &pixels, JobContext::NONE)
    }
    /// Tiles a borrowed plane, polling `job` once per tile.
    pub(crate) fn copy(
        width: u32,
        height: u32,
        pixels: &[u16],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("precision plane length differs from dimensions".into());
        }
        let tiles = TilePlane::from_contiguous_with_job(width, height, pixels, job)?;
        job.check()?;
        Ok(Self::tiled(tiles))
    }
    /// The plane holding `pixels` (`w·h` samples): sharing every tile of
    /// `self` that `pixels` leaves bitwise unchanged when the dimensions
    /// match, else [`Self::adopt`]. Always a fresh identity, even when no
    /// sample changed — a whole-plane writer's answer is an edit to its
    /// callers. A 300×300 dab on a large plane allocates at most 4 tiles.
    pub(crate) fn rebuilt(
        &self,
        width: u32,
        height: u32,
        pixels: Vec<u16>,
    ) -> Result<Self, String> {
        if (width, height) != self.dimensions() {
            return Self::adopt(width, height, pixels);
        }
        Ok(match self.tiles.rebuilt_from(&pixels, None)? {
            Some(tiles) => Self::tiled(tiles),
            None => Self::tiled(self.tiles.clone()),
        })
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.tiles.dimensions()
    }
    pub fn width(&self) -> u32 {
        self.dimensions().0
    }
    pub fn height(&self) -> u32 {
        self.dimensions().1
    }
    /// The row-major samples as one owned `Vec`. Test oracle: a library
    /// reader uses `runs`, `row_segments`, `sample_at` or `to_contiguous`.
    /// Panics when the copy cannot be allocated.
    #[doc(hidden)]
    pub fn samples(&self) -> Vec<u16> {
        self.to_contiguous()
            .expect("could not allocate typed plane storage")
    }
    /// One owned row-major copy of the samples.
    pub(crate) fn to_contiguous(&self) -> Result<Vec<u16>, String> {
        self.to_contiguous_with_job(JobContext::NONE)
    }
    /// [`Self::to_contiguous`] polling `job` once per tile.
    pub(crate) fn to_contiguous_with_job(&self, job: JobContext<'_>) -> Result<Vec<u16>, String> {
        self.tiles.to_contiguous_with_job(job)
    }
    /// One owned row-major copy polling `job` once per 4096 samples, before
    /// each block — `precision_selection_plane::copy`'s cadence over a slice,
    /// which the scalar edit's read used before 13B-4 — copying run by run.
    pub(crate) fn to_contiguous_polled(&self, job: JobContext<'_>) -> Result<Vec<u16>, String> {
        let (width, height) = self.dimensions();
        let mut out = reserved_vec(pixel_count(width, height)?)?;
        let mut visited = 0;
        for run in self.runs() {
            visited = job.every_run(visited, run.len())?;
            out.extend_from_slice(run);
        }
        Ok(out)
    }
    /// Row `y`, columns `x0..x1` (`x1` clamped to the width) as ONE slice:
    /// the tile's own run when the span lies in one tile column, else the
    /// runs copied into `scratch` (`GrayRaster::row`'s rule at 16 bits).
    pub(crate) fn row<'a>(
        &'a self,
        y: u32,
        x0: u32,
        x1: u32,
        scratch: &'a mut Vec<u16>,
    ) -> &'a [u16] {
        let mut runs = self.tiles.row_segments(y, x0, x1);
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
    /// Row `y`, columns `x0..x1`, as the tiles' own runs (`TilePlane::row_segments`).
    pub(crate) fn row_segments(&self, y: u32, x0: u32, x1: u32) -> RowSegments<'_, u16> {
        self.tiles.row_segments(y, x0, x1)
    }
    /// Every row's runs in plane order. No allocation.
    pub(crate) fn runs(&self) -> impl Iterator<Item = &[u16]> + '_ {
        let (width, height) = self.dimensions();
        (0..height).flat_map(move |y| self.tiles.row_segments(y, 0, width).map(|(_, run)| run))
    }
    /// The sample at `(x, y)`; `None` outside the plane.
    pub(crate) fn sample_at(&self, x: u32, y: u32) -> Option<u16> {
        self.tiles.sample_at(x, y)
    }
    /// `w·h·2`: the samples' bytes.
    pub fn storage_bytes(&self) -> u64 {
        let (width, height) = self.dimensions();
        u64::from(width) * u64::from(height) * 2
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }
    /// The index as `tile_structure` and every tile under `precision_plane`
    /// (a tile shared by several planes counts once).
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        self.tiles.record_resources(i, "precision_plane", false);
    }
    /// Identity (the token).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.identity, &b.identity)
    }
    /// The identity key the typed caches hold.
    pub(crate) fn key(&self) -> PrecisionPlaneKey {
        PrecisionPlaneKey {
            token: Arc::downgrade(&self.identity),
        }
    }
    /// The token's address as an opaque storage identity
    /// (`dr_precision_doc_layer_mask_storage_identity`, and the test oracles
    /// that used to compare sample addresses): equal for two planes exactly
    /// when [`Self::ptr_eq`] holds, while either is alive.
    #[doc(hidden)]
    pub fn storage_identity(&self) -> u64 {
        Arc::as_ptr(&self.identity) as usize as u64
    }
    /// Sample equality: identity first, then the samples run by run (a run
    /// of a shared tile is the same slice), polling `job` once per 4096
    /// samples up to the first differing one — the flat walk's count.
    pub(crate) fn equals(&self, other: &Self, job: JobContext<'_>) -> Result<bool, String> {
        if self.dimensions() != other.dimensions() {
            return Ok(false);
        }
        if Self::ptr_eq(self, other) {
            return Ok(true);
        }
        // Equal dimensions tile alike, so the two planes' runs pair up exactly.
        let mut visited = 0;
        for (a, b) in self.runs().zip(other.runs()) {
            if std::ptr::eq(a, b) || a == b {
                visited = job.every_run(visited, a.len())?;
                continue;
            }
            let first = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(0);
            job.every_run(visited, first + 1)?;
            return Ok(false);
        }
        Ok(true)
    }
    fn tiled(tiles: TilePlane<u16>) -> Self {
        Self {
            tiles,
            identity: Arc::new(()),
        }
    }
}
#[derive(Clone)]
pub struct PrecisionMask {
    pub(crate) plane: PrecisionPlane,
    pub(crate) properties: MaskProperties,
    pub(crate) enabled: bool,
}
impl PrecisionMask {
    pub fn new(
        plane: PrecisionPlane,
        properties: MaskProperties,
        enabled: bool,
    ) -> Result<Self, String> {
        if !properties.valid(plane.dimensions()) {
            return Err("invalid typed mask properties or padded extent".into());
        }
        // A supplied legacy property value may own byte caches. Typed masks
        // copy only its semantic fields, so no byte-derived cache crosses over.
        let mut clean = MaskProperties::at(properties.offset);
        clean.linked = properties.linked;
        clean.density = properties.density;
        clean.feather = properties.feather;
        clean.outside_coverage = properties.outside_coverage;
        Ok(Self {
            plane,
            properties: clean,
            enabled,
        })
    }
    pub fn plane(&self) -> &PrecisionPlane {
        &self.plane
    }
    pub fn properties(&self) -> &MaskProperties {
        &self.properties
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn with_properties(
        &self,
        properties: MaskProperties,
        enabled: bool,
    ) -> Result<Self, String> {
        Self::new(self.plane.clone(), properties, enabled)
    }
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        self.plane.record_resources(i);
    }
    pub(crate) fn plan(&self) -> Result<(u64, u64), String> {
        if !self.enabled || self.properties.feather == 0.0 {
            return Ok((0, 0));
        }
        let (w, h, _) = self
            .properties
            .padded_dimensions(self.plane.dimensions())
            .ok_or("invalid typed mask feather extent")?;
        let (scratch, mut work) = crate::plane_blur::float_plan(w, h, self.properties.sigma())?;
        if self.properties.outside_coverage == 255 {
            work += u64::from(w) * u64::from(h) * 2;
        }
        Ok((scratch, work))
    }
    pub(crate) fn prepare(&self, job: JobContext<'_>) -> Result<PreparedMask<'_>, String> {
        job.check()?;
        let mut out = PreparedMask {
            mask: self,
            blurred: None,
        };
        if !self.enabled || self.properties.feather == 0.0 {
            return Ok(out);
        }
        let (w, h, pad) = self
            .properties
            .padded_dimensions(self.plane.dimensions())
            .ok_or("invalid typed mask feather extent")?;
        self.plan()?;
        let mut values = reserved_vec(pixel_count(w, h)?)?;
        while values.len() < w as usize * h as usize {
            job.check()?;
            values.extend(std::iter::repeat_n(
                f32::from(self.properties.outside_coverage) / 255.0,
                (w as usize * h as usize - values.len()).min(4096),
            ));
        }
        let plane_width = self.plane.width() as usize;
        for y in 0..self.plane.height() {
            for (x0, run) in self.plane.row_segments(y, 0, self.plane.width()) {
                for (k, &sample) in run.iter().enumerate() {
                    let x = x0 + k as u32;
                    job.every(y as usize * plane_width + x as usize)?;
                    values[(y + pad) as usize * w as usize + (x + pad) as usize] =
                        f32::from(sample) / 65535.0;
                }
            }
        }
        // Blur the finite complement, because the shared reduced-resolution
        // workspace deliberately zero-extends; preserve a constant white far field.
        if self.properties.outside_coverage == 255 {
            for chunk in values.chunks_mut(4096) {
                job.check()?;
                for v in chunk {
                    *v = 1.0 - *v;
                }
            }
        }
        job.report(0.2, 6)?;
        crate::plane_blur::floats(
            &mut values,
            w,
            h,
            self.properties.sigma(),
            job.range(0.2, 0.95),
        )?;
        if self.properties.outside_coverage == 255 {
            for chunk in values.chunks_mut(4096) {
                job.check()?;
                for v in chunk {
                    *v = 1.0 - *v;
                }
            }
        }
        out.blurred = Some((values, w, h, pad));
        job.check()?;
        Ok(out)
    }
}
pub(crate) struct PreparedMask<'a> {
    mask: &'a PrecisionMask,
    blurred: Option<(Vec<f32>, u32, u32, u32)>,
}
impl PreparedMask<'_> {
    /// The mask this was prepared from: the typed plane cache's key
    /// (`precision_style_cache`).
    pub(crate) fn source(&self) -> &PrecisionMask {
        self.mask
    }
    pub(crate) fn at(&self, x: i64, y: i64) -> f32 {
        if !self.mask.enabled {
            return 1.0;
        }
        let properties = &self.mask.properties;
        let px = x - i64::from(properties.offset.0);
        let py = y - i64::from(properties.offset.1);
        let outside = f32::from(properties.outside_coverage) / 255.0;
        let coverage = if let Some((values, w, h, pad)) = &self.blurred {
            let px = px + i64::from(*pad);
            let py = py + i64::from(*pad);
            if px >= 0 && py >= 0 && px < i64::from(*w) && py < i64::from(*h) {
                values[py as usize * *w as usize + px as usize]
            } else {
                outside
            }
        } else if px >= 0 && py >= 0 && px <= i64::from(u32::MAX) && py <= i64::from(u32::MAX) {
            self.mask
                .plane
                .sample_at(px as u32, py as u32)
                .map_or(outside, |sample| f32::from(sample) / 65535.0)
        } else {
            outside
        };
        density_coverage(coverage, properties.density, 1.0)
    }
}
static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone)]
pub struct PrecisionChannel {
    pub(crate) id: u64,
    pub(crate) name: Arc<String>,
    pub(crate) plane: PrecisionPlane,
    pub(crate) metadata: crate::precision_channel_metadata::ChannelMetadata,
}
impl PrecisionChannel {
    pub fn new(name: &str, plane: PrecisionPlane) -> Result<Self, String> {
        if name.len() > MAX_DRDC_NAME_LEN as usize {
            return Err("typed channel name exceeds native limit".into());
        }
        Ok(Self {
            id: NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed),
            name: Arc::new(name.into()),
            plane,
            metadata: Default::default(),
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn plane(&self) -> &PrecisionPlane {
        &self.plane
    }
    pub fn metadata(&self) -> &crate::precision_channel_metadata::ChannelMetadata {
        &self.metadata
    }
    pub(crate) fn duplicate(&self) -> Self {
        let mut next = self.clone();
        next.id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed);
        next
    }
    pub(crate) fn record_resources(&self, i: &mut Inventory) {
        i.object(&*self.name, "precision_channel", false);
        i.string(&self.name, "precision_channel", false);
        if let Some(text) = &self.metadata.provenance {
            i.object(&**text, "precision_channel_provenance", false);
            i.string(text, "precision_channel_provenance", false);
        }
        self.plane.record_resources(i);
    }
}
