//! The CMYK document's raster storage (#1 phase 1): [`CmykImage`], an
//! immutable straight CMYKA16 plane over the shared tile store
//! (`TilePlane<[u16; 5]>`), 10 bytes a pixel.
//!
//! A sample is `[c, m, y, k, a]`: each ink 0 = no ink, 65535 = 100 %, and
//! alpha STRAIGHT (coverage, never multiplied into the inks). Ink is stored
//! as ink, not as the inverted "dot gain" form PSD and some TIFF writers use;
//! the readers and writers in `cmyk_io` flip at the file boundary only.
//!
//! The image is pure: every constructor mints a fresh identity and an empty
//! alpha-fact cell (`minted`), clones share the tiles, the identity and the
//! cell, and nothing mutates a built plane. The identity (`identity_key`) is
//! what the Layers panel's thumbnail cache and
//! `dr_precision_doc_cmyk_layer_identity` key on, `PrecisionImage`'s twin.

use std::sync::{Arc, OnceLock};

use crate::job_control::JobContext;
use crate::precision_image::{admit, pixel_count, process_working_limit, BOOKKEEPING_BYTES};
use crate::resources::Inventory;
use crate::tile_store::{RowSegments, TilePlane};

/// One stored sample: C, M, Y, K, A, each 0…65535.
pub type CmykSample = [u16; 5];

/// Bytes a pixel of CMYKA16 storage (five u16 components).
pub const CMYK_BYTES_PER_PIXEL: u64 = 10;

/// The fully transparent, inkless sample — paper with nothing on it.
pub const PAPER: CmykSample = [0, 0, 0, 0, 0];

#[derive(Clone)]
pub struct CmykImage {
    plane: TilePlane<CmykSample>,
    identity: Arc<()>,
    /// The flat-save list's alpha facts of `plane` (`alpha_facts_once`).
    alpha: Arc<OnceLock<(bool, bool)>>,
}

impl CmykImage {
    /// A fresh identity and an empty alpha-fact cell over `plane`: every
    /// constructor's image.
    fn minted(plane: TilePlane<CmykSample>) -> Self {
        Self {
            plane,
            identity: Arc::new(()),
            alpha: Arc::new(OnceLock::new()),
        }
    }

    /// Builds the image row by row: `fill(y, row)` writes row `y`
    /// (`row.len() == width`). Admits the plane's `10·w·h` bytes beside
    /// bookkeeping first, against the process working limit, the way every
    /// decoded `PrecisionImage` constructor admits.
    pub fn from_rows(
        width: u32,
        height: u32,
        fill: impl FnMut(u32, &mut [CmykSample]) -> Result<(), String>,
    ) -> Result<Self, String> {
        Self::from_rows_with_job(width, height, fill, JobContext::NONE)
    }

    pub(crate) fn from_rows_with_job(
        width: u32,
        height: u32,
        fill: impl FnMut(u32, &mut [CmykSample]) -> Result<(), String>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let bytes = pixel_count(width, height)? as u64 * CMYK_BYTES_PER_PIXEL;
        admit(bytes, BOOKKEEPING_BYTES, 0, process_working_limit())?;
        let plane = TilePlane::from_rows_shared_with_job(width, height, fill, job)?;
        Ok(Self::minted(plane))
    }

    /// Tiles a row-major contiguous plane (`samples.len() == w·h`), minting
    /// a fresh identity: the adoption of a geometry walk's contiguous output
    /// (#1 phase 4b), `PrecisionImage::from_rgba16_unseeded`'s twin. The
    /// caller's plan charges the copy (`cmyk_adoption_copy_bytes`).
    pub(crate) fn from_contiguous(
        width: u32,
        height: u32,
        samples: &[CmykSample],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Ok(Self::minted(TilePlane::from_contiguous_with_job(
            width, height, samples, job,
        )?))
    }

    /// `value` everywhere: at most four tile allocations.
    pub fn filled(width: u32, height: u32, value: CmykSample) -> Result<Self, String> {
        Ok(Self::minted(TilePlane::filled(width, height, value)?))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.plane.dimensions()
    }

    /// Row `y`, columns `x0..x1`, as the tiles' own runs (no allocation).
    pub fn row_segments(&self, y: u32, x0: u32, x1: u32) -> RowSegments<'_, CmykSample> {
        self.plane.row_segments(y, x0, x1)
    }

    /// The sample at `(x, y)`; `None` outside the image.
    pub fn sample_at(&self, x: u32, y: u32) -> Option<CmykSample> {
        self.plane.sample_at(x, y)
    }

    /// The row-major samples of a rect inside the image (`TilePlane`'s
    /// rect read, polling `job` per visited tile); `Err` when the rect leaves
    /// the image or its copy cannot be allocated.
    pub(crate) fn read_rect(
        &self,
        (x, y, w, h): (u32, u32, u32, u32),
        job: JobContext<'_>,
    ) -> Result<Vec<CmykSample>, String> {
        self.plane
            .read_rect_with_job(x, y, w, h, job)?
            .ok_or_else(|| "could not read the CMYK rect".to_string())
    }

    /// The image with a rect overwritten by `samples` (row-major, stride
    /// `w`), sharing every bitwise-unchanged tile and minting a new
    /// identity; `Ok(None)` when no tile changed (the paint walk's purity
    /// latch).
    pub(crate) fn with_rect(
        &self,
        (x, y, w, h): (u32, u32, u32, u32),
        samples: &[CmykSample],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        Ok(self
            .plane
            .with_rect_with_job(x, y, w, h, samples, job)?
            .map(Self::minted))
    }

    /// Bitwise sample equality (the tile store's only equality).
    pub fn same_samples(&self, other: &Self) -> bool {
        self.plane.same_bits(&other.plane)
    }

    /// `PrecisionImage::alpha_facts_once`'s twin: `scan` runs on the first
    /// ask and every clone shares its answer; a written plane is a new image
    /// (`minted`), whose cell starts empty.
    pub(crate) fn alpha_facts_once(&self, scan: impl FnOnce() -> (bool, bool)) -> (bool, bool) {
        *self.alpha.get_or_init(scan)
    }

    /// Whether [`Self::alpha_facts_once`] has answered for these samples.
    pub(crate) fn alpha_facts_known(&self) -> bool {
        self.alpha.get().is_some()
    }

    /// Identity: the same construction (clones included).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.identity, &b.identity)
    }

    /// The identity as a number, stable while the image lives.
    pub(crate) fn identity_key(&self) -> u64 {
        Arc::as_ptr(&self.identity) as usize as u64
    }

    /// A `Weak` of the identity token: the content-box memo's key
    /// (`precision_content_box`), `PrecisionImage::identity_key`'s twin.
    pub(crate) fn identity_weak(&self) -> std::sync::Weak<()> {
        Arc::downgrade(&self.identity)
    }

    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }

    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        self.plane
            .record_resources(inventory, "layer_pixels", false);
    }

    /// Every sample, row-major (tests and the encoders' row loops read
    /// `row_segments` instead; this is the oracle's whole-plane read).
    pub fn to_contiguous(&self) -> Result<Vec<CmykSample>, String> {
        self.plane.to_contiguous(None)
    }
}
