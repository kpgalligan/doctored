//! `LayerRaster` — the ONE accessor for a byte layer's pixels (13B-2a, tiled
//! since 13B-2b).
//!
//! `Layer.pixels`, `RetainedSource8.rendered` and `WarpBinding8`'s
//! `source_pixels` / `rendered_pixels` share allocation identity with each
//! other (a fallback is "still current" exactly when the layer holds the very
//! allocation the source rendered), so all four are this one type. Its
//! representation is private. Since 13B-2b it is one of two:
//!
//! - **Tiled** — a `tile_store::TilePlane<[u8; 4]>`. [`LayerRaster::new`]
//!   tiles its image ([`LayerRaster::unseeded`] too, without seeding the
//!   view cache — for the planes nothing reads whole: `rendered_group`'s
//!   synthetic, the `.rz` asset reader, the `.rz` layer reader, the WARP
//!   original reader, the converted and resampled renders, the filtered
//!   cache output, Place's 1×1 placeholder, the eight geometry outputs —
//!   `transform_layer`'s exact and resample arms, the geometry set's plain
//!   arm, `perspective_layer`, `geometry` rotate/flip, `resize`,
//!   `doc_warp::render_pixels` and the retouch expansion — and the two
//!   retouch temp documents);
//!   [`LayerRaster::rebuilt`] (every whole-image writer) shares each
//!   bitwise-unchanged tile with the raster it replaces and
//!   [`LayerRaster::with_rect`] allocates only the tiles its rect changes, so
//!   a 300×300 dab on a 24 MP layer retains ≤ 4 new tiles instead of a whole
//!   copy. The rect writers (13B-2c-2a, `layer_rect`) read only their rect
//!   through `read_rect` and write it back through `with_rect`: the two paint
//!   writers, heal, inpaint, `with_layer_plane`, `painting_layer_plane`,
//!   dodge/burn, bucket fill, Clear, the retouch-output deposit and red-eye —
//!   none seeds a view. [`LayerRaster::filled`] (13B-3d-3b, the narrowing's
//!   masked-adjustment plane; also the transparent layer
//!   `adding_layer` / `adding_layer_at` insert) is one repeated sample over
//!   `TilePlane::filled` — one tile per tile shape — and seeds nothing.
//! - **Contiguous** — 13B-2a's exact `Arc<RgbaImage>`. Since 13B-2c-2c only
//!   the `.rz` reader's 0×0 placeholder and the test oracles (the contiguous
//!   twins) are adopted this way through [`LayerRaster::shared`]; every
//!   source asset (`SourcePixels::Rgba8`) and every rendered source tiles, so
//!   a placed layer IS its asset's tile plane under a fresh identity. `new`
//!   falls back to it when the store refuses the image (zero area, above
//!   `MAX_PIXELS`, or an allocation failure) — never a silent loss.
//!
//! The contract:
//!
//! - **Identity** is `ptr_eq` / `key()` / `weak_count` — a private
//!   `Arc<()>` token minted by `new` / `shared` / `rebuilt` / `with_rect`
//!   (`Some`) / `reidentified` and shared by clones, never the bytes and
//!   never a tile. The two identity caches (the layer-style plane cache and
//!   the group content-box memo) key on [`LayerRasterKey`], a `Weak` of that
//!   token. A whole-image writer answers a FRESH identity even when every
//!   tile is shared (a no-change `rebuilt` is still an edit to its callers).
//! - **No `Deref<Target = RgbaImage>`.** A permanent deref pins a contiguous
//!   buffer. There is no `PartialEq` either: bytes equality on tiles is a
//!   whole-plane walk and identity equality would read as bytes. Compare
//!   `same_bytes` (raster to raster) or `equals_image` (raster to an owned
//!   image) when bytes are meant, `ptr_eq` when identity is; neither touches
//!   the view cache.
//! - **Bind once.** [`LayerRaster::contiguous_image`] is the legacy
//!   whole-image view; since 13B-2c-3b it has no production caller (the
//!   residual is 0) and serves the test oracles. It is called at most once
//!   per raster per operation, at the top of the function or block that owns
//!   the read — never inside a loop body or a per-row / per-sample closure —
//!   because every call on a tiled raster is a cache lookup and a miss is a
//!   whole-plane materialization.
//! - **The view cache.** A tiled raster's view lives in ONE process-wide
//!   bounded cache (`contiguous_views`, shared with the typed
//!   `PrecisionImage` since 13B-3b-1; `CONTIGUOUS_CACHE_BYTES` /
//!   `CONTIGUOUS_CACHE_ENTRIES`,
//!   least recently used evicted first), keyed on the tile plane's INDEX
//!   allocation (`TilePlane::weak_index`) — never on the identity token, so
//!   holding a view does not move `weak_count` — with at most one live entry
//!   per plane. `new` and `rebuilt` seed it with the image they were handed,
//!   so the first read after a whole-image write is free. The lock never
//!   covers a materialization: a miss builds the view outside it and
//!   re-checks before inserting, so concurrent readers of one plane end with
//!   one shared view. Since 13B-2c-1 the compositor, the regional per-sample
//!   tree and the content-box scan read the tiles in place
//!   ([`LayerRaster::row_segments`] / [`LayerRaster::sample_at`]), so a
//!   projection or a regional window leaves no view behind; since
//!   13B-2c-3a the `.rz` / PSD / asset / WARP writers, the flat-save loss
//!   scan, the thumbnail, `layer_canvas_image`, the sampling-plate copy and
//!   the 16-bit promotion read through [`LayerRaster::runs`] /
//!   `row_segments` / `to_contiguous` too, so a save, an export, a
//!   thumbnail or a reopen leaves none either; since 13B-2c-3b the
//!   resample inputs (transform, perspective, geometry, resize, warp and
//!   the source render) read a transient
//!   `to_contiguous[_with_job]` copy and the profile conversion, the
//!   retouch expansion, the scoped analysis and the retouch temp documents
//!   read `runs` / `row_segments` / `sample_at` / `read_rect`, so no
//!   production read binds a view; the bound (one projection's working set,
//!   a 3-layer 48 MP or 10-layer 24 MP document) serves the seeds of `new` /
//!   `rebuilt` and the test oracles, and 13B-6 absorbs it. The views are
//!   DISPOSABLE:
//!   the resource report lists the views of the supplied documents' planes
//!   as `contiguous_views` cache bytes and `dr_mixed_resources_evict_caches` drops
//!   every view.
//! - `to_contiguous` is the owned copy writers mutate; `read_rect` has
//!   `tile_store::TilePlane::read_rect` semantics with a u64 bounds check;
//!   `row_segments` / `sample_at` / `runs` are the allocation-free in-place
//!   reads (`TilePlane`'s rules on both representations).
//!
//! Since 13B-2b its private representation is `Tiled(TilePlane<[u8;4]>)` or
//! `Contiguous(Arc<RgbaImage>)` (2a's, taken by `shared` — which adopts, never copies, until the
//! first write tiles it — and by `new` when the tile store refuses: zero area, above `MAX_PIXELS`,
//! allocation failure). Since 13B-2c-2c every PRODUCER tiles: a byte source asset
//! (`SourcePixels::Rgba8`) holds a `LayerRaster` (Place ADOPTS the input layer's raster, identity
//! included; the `.rz` reader builds it `unseeded`), a render answers a fresh identity over the
//! asset's tiles (`Colored8::into_raster` → `reidentified`) or an `unseeded` resample, so a placed
//! layer IS its asset's plane and counts once, before and after its first edit — `shared`'s only
//! production caller left is the `.rz` reader's 0×0 placeholder (the contiguous twins are test
//! oracles). Identity (`ptr_eq` / `key` / `weak_count`, and `storage_identity` — the token's
//! address as a `u64`, what `dr_doc_layer_storage_identity` exports) is a private `Arc<()>`
//! token minted by `new` / `shared` / `rebuilt` / `with_rect` (`Some`) / `reidentified`, never the
//! bytes or a tile. Whole-image writers go through `rebuilt(image)` (shares every bitwise-unchanged
//! tile, fresh identity even for equal bytes — `with_layer_pixels` relies on that), the two paint
//! writers through `read_rect` + `with_rect` over the overlay's nonzero rect
//! (`doc_paint::overlay_rect`), so a 300×300 dab retains ≤ 4 new tiles. Since 13B-2c-2a the RECT
//! WRITERS — heal (the union of its component windows), content-aware/spot fill (the union of its
//! plans' windows), `with_layer_plane` (`canvas_clip`), `painting_layer_plane` and dodge/burn
//! (`overlay_rect`), bucket fill (the grown region's bbox; `doc_select::grow_region` takes
//! dimensions + a sample closure, so the region grows over `sample_at`), Clear (`coverage_rect`),
//! the retouch-output deposit (the domain's bbox) and red-eye (its canvas rect on the layer, one
//! read shared by the coverage and write passes) — read only that rect through `layer_rect.rs`
//! (`canvas_clip` / `canvas_rect_on_layer` / `coverage_rect` / `nonzero_rect` / `rect_image` /
//! `rect_offset`, all layer-space `(x, y, w, h)`; since 13B-3d-1 each byte form delegates to its
//! PLANE form — `plane_clip` / `canvas_rect_on_plane` / `coverage_rect16` (one generic row scan
//! with `coverage_rect`) / `plane_rect_offset`, over a plane's dimensions and canvas offset — which
//! the typed paint writers call) and write it back through `with_rect`, so none seeds a view; the
//! rect bounds the READ, `with_rect`'s bitwise diff bounds the copies. Each keeps its answers:
//! `with_rect` `Ok(None)` maps to the site's old no-change answer (NULL, or `reidentified` where
//! the old path answered a document — bucket fill, Clear), and the one stated narrowing is
//! inpaint's union window that misses the layer answering `Ok(None)` before any solve. Heal checks
//! every part's `over_window_limit` BEFORE the rect. Whole-image writers (`gradient`,
//! `apply_layer_pixels` / `apply_layer_plane`, `layer_via`, mask apply, `doc_color_policy`,
//! `dr_doc_layer_image`) stay on `to_contiguous` + `rebuilt` by decision. Since 13B-2c-1 the byte
//! compositor (`composite_layer_into`, `composite_pixels_into`, the knockout unit pass,
//! `Shape::of_layer`, the cutout's opaque precondition) reads rows through `row_segments` (the
//! layer-space span computed ONCE per layer and `(0, 0)` unless the canvas span is non-empty, so no
//! negative is cast; `mi` is always the PLANE index `ly·lw + lx`, `li = i·4` only ever indexes
//! `run.as_flattened()`), and the regional per-sample tree (`ResolvedNode::pixel`) and
//! `scan_content_box` read `sample_at` — nothing is bound, so a projection or a regional window
//! leaves no view behind. Since 13B-2c-3a the persistence/export/preservation reads leave no view
//! either: the `.rz` layer PNG and asset PNG writers encode from a transient `to_contiguous()`
//! copy, the WARP original is written through `runs()` (every row's runs in plane order), the PSD
//! writer splits its planes over `runs()`, the flat-save loss scans walk `runs().flatten()`, the
//! thumbnail reads a transient `to_contiguous()` (only below the box threshold — a
//! larger layer's thumbnail reads its rows, `layer_thumbnail`), `layer_canvas_image` and the
//! sampling-plate `copy_layer` copy rows through `canvas_clip` + `row_segments`, and the 16-bit
//! structure promotion goes through `promote_raster`; the `.rz` layer reader and the WARP original
//! reader build `unseeded`, and a reopened placed layer whose bytes equal its 8-bit asset ADOPTS
//! the asset's plane (`materialize_byte`: `same_bytes` → `reidentified`), so it shares every tile
//! as at place time. Since 13B-2c-3b the residual is **0**: the tap-walk resample inputs
//! (`transform_layer`'s exact and resample arms, `perspective_layer`, the geometry set's plain arm,
//! `geometry` rotate/flip, `resize`, `doc_warp::render_pixels`, `render8`'s Sample arm)
//! bind ONE transient owned copy (`to_contiguous_with_job(job)?`
//! where a job is in hand, else `to_contiguous().ok()?`) dropped with its block — a per-tap tile
//! lookup would slow the interactive transform inner loop — so an allocation failure answers `None`
//! / `ALLOCATION` instead of panicking; the profile conversion walks `runs().flatten()`, the
//! retouch expansion copies rows through `row_segments`, the byte scoped analysis reads
//! `sample_at`, and both retouch temp documents are
//! `DrDocument::from_raster(LayerRaster::unseeded(plate))` read back through `runs()`
//! (content-aware) or `read_rect` of the domain rect (`deposit_into` takes `&LayerRaster`). The
//! accepted cost is the WARM path: a repeated edit on one input plane (a second `warp_edit`
//! re-reads `binding.source_pixels`) pays a full transient copy per edit where it used to hit the
//! view cache — measured by `phase13b_tile_sharing`'s `transforms/warm` rows (6 MP: `warp_edit#2`
//! 699.6 → 686.2 ms, one sample; views held 72064016 bytes → 0); it missed no bound, so no
//! mitigation is planned (dropped 2026-10-30, the 13B status). `contiguous_image()` (no
//! production caller since 13B-2c-3b — the cache's read path, kept for the test oracles and
//! 13B-6; an `Arc` BY VALUE) is served from ONE process-wide bounded view cache (since 13B-3b-1
//! in `contiguous_views.rs`, shared with the typed `PrecisionImage`; `layer_raster` re-exports
//! `contiguous_view_bytes` and the two bounds; `CONTIGUOUS_CACHE_BYTES` 1
//! GiB = one projection's working set, `CONTIGUOUS_CACHE_ENTRIES` 256, LRU, one live entry per
//! plane, keyed on the plane INDEX via `TilePlane::weak_index` so `weak_count` is unaffected,
//! seeded by `new` / `rebuilt` — `LayerRaster::unseeded` is `new` without the seed, for the planes
//! nothing reads whole: `rendered_group`'s synthetic (read only through `row_segments` /
//! `sample_at`), the `.rz` asset reader, the `.rz` layer reader, the WARP original reader, the
//! converted plane (`Colored8::into_raster`), the resampled render, the filtered cache output,
//! `render8`'s quantized 16-bit render (13B-3e-2), Place's 1×1 placeholder, the eight geometry
//! outputs (`transform_layer`'s exact and resample arms, the geometry set's plain arm,
//! `perspective_layer`, `geometry` rotate/flip, `resize`, `doc_warp::render_pixels`, the retouch
//! expansion) and the two retouch temp documents — materialized outside the lock with a re-check);
//! a report records the views of the supplied documents' planes as `contiguous_views` cache bytes
//! (top-level `contiguous_view_bytes`; `contiguous_views_of(walked)` clones them under the lock,
//! `record_contiguous_views` pins and records them, and eviction scopes before/after with ONE walk)
//! and `dr_mixed_resources_evict_caches` drops every view. It is still BOUND ONCE per raster per
//! operation, at the top of the owning function or block — never inside a loop body or a
//! per-row/per-sample closure; a per-sample reader uses `sample_at` instead. NO `Deref<Target =
//! RgbaImage>` and NO `PartialEq` — since 13B-2c-2b compare `same_bytes` (raster to raster: tiles
//! through `TilePlane::same_bits`, a mixed pair through the TILED side's `equals_raw` over the
//! contiguous side's image; symmetric) or `equals_image` (raster to an owned image, its first
//! `w·h·4` bytes) when bytes are meant, `ptr_eq` when identity is; neither touches the view cache
//! (the five former `*contiguous_image()` comparison sites — `doc_rotate::exact_geometry_changed`,
//! `warp_native::attach_byte`, which now compares the raw original BEFORE building a raster,
//! `artwork_document`, `doc_content`, `doc_lock::freezing_alpha`, whose restore reads `before`
//! through `row_segments` — use them). `to_contiguous_with_job(job)` (`pub(crate)`,
//! `doc_color_policy`'s per-layer conversion) is the job-polled whole-layer copy: once per tile
//! (tiled), once before the copy (contiguous); `to_contiguous` is it with `JobContext::NONE`. A
//! cancel returns the control's own sentence and an allocation failure `ALLOCATION` —
//! `materialize_with_job` tells them apart with `JobContext::cancelled` (a flag read, not a
//! checkpoint), never by matching text. `byte_len()` answers `w·h·4` without a view;
//! `shared_tile_count` is the test oracle. `LayerRaster::filled(w, h, value)` (`pub(crate)`,
//! 13B-3d-3b) is one repeated sample over `TilePlane::filled` — one tile per tile shape, NO seed,
//! `new`'s contiguous fallback when the store refuses — the narrowing's masked-adjustment plane
//! (`precision_conversion_narrow`). Both identity caches (`style_cache`, `doc_group_query`'s
//! content-box memo) key on `LayerRasterKey` (`key` / `same` / `is_dead`), never on a materialized
//! `Arc`; `record_resources` records the index as `tile_structure` and the tiles under the caller's
//! category (contiguous: 2a's two records).

use std::sync::{Arc, Weak};

use image::{Rgba, RgbaImage};

use crate::contiguous_views::{self, PlaneKey, View};
use crate::job_control::JobContext;
use crate::resources::Inventory;
use crate::tile_store::{RowSegments, TilePlane};

pub use crate::contiguous_views::{
    contiguous_view_bytes, CONTIGUOUS_CACHE_BYTES, CONTIGUOUS_CACHE_ENTRIES,
};

/// The allocation-failure sentence of every layer-raster copy: the rect
/// writers raise it too (`doc_heal` / `doc_inpaint`); a cancelled job-polled
/// copy returns the control's sentence instead — `materialize_with_job` tells
/// the two apart with `JobContext::cancelled`, never by matching text.
pub(crate) const ALLOCATION: &str = "could not allocate layer raster storage";

/// A byte layer's straight-alpha RGBA8 pixels, immutable and shared.
#[derive(Clone)]
pub struct LayerRaster {
    repr: Repr,
    identity: Arc<()>,
}

#[derive(Clone)]
enum Repr {
    Tiled(TilePlane<[u8; 4]>),
    Contiguous(Arc<RgbaImage>),
}

/// The exact `w·h` samples of `image` (an `RgbaImage` buffer may be longer).
fn samples(image: &RgbaImage) -> &[[u8; 4]] {
    let len = (image.width() as usize * image.height() as usize * 4).min(image.as_raw().len());
    image.as_raw()[..len].as_chunks::<4>().0
}

impl LayerRaster {
    /// Takes ownership of `image` as a fresh, uniquely identified raster:
    /// tiled, with `image` itself seeding the view cache; contiguous when the
    /// tile store refuses it.
    pub fn new(image: RgbaImage) -> Self {
        let (width, height) = image.dimensions();
        match TilePlane::from_contiguous(width, height, samples(&image), None) {
            Ok(plane) => {
                seed(&plane, image);
                Self::tiled(plane)
            }
            Err(_) => Self::contiguous(Arc::new(image)),
        }
    }

    /// Exactly [`Self::new`] without the view-cache seed (the same
    /// contiguous fallback): for a raster nobody reads whole a seed would
    /// only evict a live view. Its callers: the `rendered_group` synthetic
    /// (read only through `row_segments` / `sample_at`), the `.rz` asset
    /// reader, the `.rz` layer reader and the WARP original reader (an open
    /// seeds no view), the converted plane (`Colored8::into_raster`), the
    /// resampled render, the filtered cache output, Place's 1×1
    /// placeholder, the eight geometry outputs (`transform_layer`'s exact and
    /// resample arms, the geometry set's plain arm, `perspective_layer`,
    /// `geometry` rotate/flip, `resize`, `doc_warp::render_pixels`, the
    /// retouch expansion), the two retouch temp documents
    /// (`DrDocument::from_raster(LayerRaster::unseeded(plate))`) and, since
    /// 13B-3e-4b, the 16→8 / float→8 narrowing's base raster, WARP original
    /// and 1×1 placeholders (`precision_conversion_narrow`).
    pub fn unseeded(image: RgbaImage) -> Self {
        let (width, height) = image.dimensions();
        match TilePlane::from_contiguous(width, height, samples(&image), None) {
            Ok(plane) => Self::tiled(plane),
            Err(_) => Self::contiguous(Arc::new(image)),
        }
    }

    /// A `width`×`height` raster of `value` everywhere over
    /// `TilePlane::filled` (one tile per tile shape, 13B-3d-3b), no seed; the
    /// contiguous fallback of [`Self::new`] when the store refuses it.
    pub(crate) fn filled(width: u32, height: u32, value: [u8; 4]) -> Self {
        match TilePlane::filled(width, height, value) {
            Ok(plane) => Self::tiled(plane),
            Err(_) => Self::contiguous(Arc::new(RgbaImage::from_pixel(width, height, Rgba(value)))),
        }
    }

    /// Adopts an existing shared image without copying (13B-2a's
    /// representation); its first write tiles it. Since 13B-2c-2c the only
    /// production caller is the `.rz` reader's 0×0 placeholder — source
    /// assets and renders tile — and the contiguous-twin test oracles.
    pub fn shared(image: Arc<RgbaImage>) -> Self {
        Self::contiguous(image)
    }

    /// The raster holding `image`, sharing every tile of `self` that `image`
    /// leaves bitwise unchanged (same dimensions, tiled `self`), else
    /// [`Self::new`]. Always a fresh identity, even when nothing changed.
    pub fn rebuilt(&self, image: RgbaImage) -> Self {
        let Repr::Tiled(plane) = &self.repr else {
            return Self::new(image);
        };
        if plane.dimensions() != image.dimensions() {
            return Self::new(image);
        }
        let next = match plane.rebuilt_from(samples(&image), None) {
            Ok(Some(next)) => next,
            Ok(None) => plane.clone(),
            Err(_) => return Self::new(image),
        };
        seed(&next, image);
        Self::tiled(next)
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
        samples: &[[u8; 4]],
    ) -> Result<Option<Self>, String> {
        self.with_rect_with_job(x, y, w, h, samples, JobContext::NONE)
    }

    /// [`Self::with_rect`] polling `job`: once per visited tile (tiled,
    /// `TilePlane::with_rect_with_job`), once per rect row of the compare and
    /// of the copy plus [`Self::to_contiguous_with_job`]'s own check
    /// (contiguous). Err: the control's own sentence for a cancel.
    pub(crate) fn with_rect_with_job(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[[u8; 4]],
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
        let stride = width as usize * 4;
        let span = w as usize * 4;
        let at = |row: usize| (y as usize + row) * stride + x as usize * 4;
        let raw = image.as_raw();
        let rows = samples.chunks_exact(w as usize);
        let mut same = true;
        for (row, line) in rows.clone().enumerate() {
            job.check()?;
            if raw[at(row)..at(row) + span] != *line.as_flattened() {
                same = false;
                break;
            }
        }
        if same {
            return Ok(None);
        }
        let mut copy = self.to_contiguous_with_job(job)?;
        let target: &mut [u8] = &mut copy;
        for (row, line) in rows.enumerate() {
            job.check()?;
            target[at(row)..at(row) + span].copy_from_slice(line.as_flattened());
        }
        Ok(Some(Self::new(copy)))
    }

    /// The same pixels in the same representation under a fresh identity
    /// (a writer whose edit changed no sample still answers a new raster).
    /// Every tile is shared, so a render that places an asset unchanged
    /// (`retained_source_render::Colored8::into_raster`) IS the asset's plane
    /// and counts once.
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

    /// `w·h·4`, the pixel payload in bytes, without touching a view.
    pub fn byte_len(&self) -> u64 {
        let (width, height) = self.dimensions();
        u64::from(width) * u64::from(height) * 4
    }

    /// Tiles `self` and `other` share by pointer at the same index; 0 unless
    /// both are tiled with equal dimensions. Test oracle.
    pub fn shared_tile_count(&self, other: &Self) -> usize {
        match (&self.repr, &other.repr) {
            (Repr::Tiled(a), Repr::Tiled(b)) => a.shared_tile_count(b),
            _ => 0,
        }
    }

    /// Whether `self` holds exactly `other`'s sample storage: equal
    /// dimensions and either the same contiguous `Arc` or every tile shared by
    /// pointer. Pointer identity only, never a byte compare
    /// (`drdc_reopen_plan`'s adoption term).
    pub(crate) fn shares_samples(&self, other: &Self) -> bool {
        if self.dimensions() != other.dimensions() {
            return false;
        }
        match (&self.repr, &other.repr) {
            (Repr::Contiguous(a), Repr::Contiguous(b)) => Arc::ptr_eq(a, b),
            (Repr::Tiled(a), Repr::Tiled(b)) => {
                a.shared_tile_count(b) == a.columns() as usize * a.rows() as usize
            }
            _ => false,
        }
    }

    /// The legacy whole-image view: the adopted Arc of a contiguous raster,
    /// else the cached (or freshly materialized and cached) view of the
    /// tiles. No production caller since 13B-2c-3b; the cache's read path,
    /// kept for the test oracles (48 test files) and 13B-6.
    /// Bind ONCE per raster per operation, outside every loop and per-sample
    /// closure (module doc). An allocation failure while materializing
    /// panics ("could not allocate layer raster storage"), which the FFI's
    /// `catch_unwind` reports as an internal error.
    pub fn contiguous_image(&self) -> Arc<RgbaImage> {
        match &self.repr {
            Repr::Tiled(plane) => view(plane),
            Repr::Contiguous(image) => Arc::clone(image),
        }
    }

    /// An owned copy of the pixels, admitted through
    /// `precision_image::reserved_vec`. Err: "could not allocate layer raster
    /// storage".
    pub fn to_contiguous(&self) -> Result<RgbaImage, String> {
        self.to_contiguous_with_job(JobContext::NONE)
    }

    /// [`Self::to_contiguous`] polling `job`: once per tile (tiled), once
    /// before the copy (contiguous). Err: [`ALLOCATION`] for an allocation
    /// failure, the control's own sentence for a cancel.
    pub(crate) fn to_contiguous_with_job(&self, job: JobContext<'_>) -> Result<RgbaImage, String> {
        match &self.repr {
            Repr::Tiled(plane) => materialize_with_job(plane, job),
            Repr::Contiguous(image) => {
                job.check()?;
                let raw = image.as_raw();
                let mut samples = crate::precision_image::reserved_vec::<u8>(raw.len())
                    .map_err(|_| ALLOCATION.to_string())?;
                samples.extend_from_slice(raw);
                RgbaImage::from_raw(image.width(), image.height(), samples)
                    .ok_or_else(|| ALLOCATION.to_string())
            }
        }
    }

    /// Bytes equality with `other`: `false` when dimensions differ; tiles
    /// compare through `TilePlane::same_bits`, a tiled raster against a
    /// contiguous one through the tiled side's [`Self::equals_raw`], two
    /// contiguous rasters by `Arc::ptr_eq` or else their `w·h` samples equal.
    /// Symmetric; never touches the view cache.
    pub fn same_bytes(&self, other: &Self) -> bool {
        if self.dimensions() != other.dimensions() {
            return false;
        }
        match (&self.repr, &other.repr) {
            (Repr::Tiled(a), Repr::Tiled(b)) => a.same_bits(b),
            (Repr::Contiguous(a), Repr::Contiguous(b)) => {
                Arc::ptr_eq(a, b) || samples(a) == samples(b)
            }
            (Repr::Tiled(_), Repr::Contiguous(image)) => {
                self.equals_raw(image.dimensions(), image.as_raw())
            }
            (Repr::Contiguous(image), Repr::Tiled(_)) => {
                other.equals_raw(image.dimensions(), image.as_raw())
            }
        }
    }

    /// Bytes equality with an owned image (its first `w·h·4` bytes). Never
    /// touches the view cache.
    pub fn equals_image(&self, image: &RgbaImage) -> bool {
        self.equals_raw(image.dimensions(), image.as_raw())
    }

    /// Bytes equality with `raw` of `dimensions`: `false` unless the
    /// dimensions match and `raw` holds at least `w·h·4` bytes; compares
    /// exactly the first `w·h·4` (the `samples()` rule). No view.
    pub(crate) fn equals_raw(&self, dimensions: (u32, u32), raw: &[u8]) -> bool {
        if dimensions != self.dimensions() {
            return false;
        }
        let len = dimensions.0 as usize * dimensions.1 as usize * 4;
        if raw.len() < len {
            return false;
        }
        let chunks = raw[..len].as_chunks::<4>().0;
        match &self.repr {
            Repr::Tiled(plane) => plane.equals_contiguous(chunks),
            Repr::Contiguous(image) => samples(image) == chunks,
        }
    }

    /// `tile_store::TilePlane::read_rect` semantics: row-major samples;
    /// zero-area → `Some(vec![])`; a rect that leaves the plane → `None`.
    /// Bounds are checked in u64, so no `x + w` can wrap. `None` also when
    /// the copy cannot be allocated.
    pub fn read_rect(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<[u8; 4]>> {
        self.read_rect_with_job(x, y, w, h, JobContext::NONE)
            .ok()
            .flatten()
    }

    /// [`Self::read_rect`] polling `job`: once per visited tile
    /// (tiled, `TilePlane::read_rect_with_job`), once per rect row
    /// (contiguous). `Ok(None)` is `read_rect`'s `None`; `Err` is the
    /// control's cancel sentence.
    pub(crate) fn read_rect_with_job(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        job: JobContext<'_>,
    ) -> Result<Option<Vec<[u8; 4]>>, String> {
        let (width, height) = self.dimensions();
        if u64::from(x) + u64::from(w) > u64::from(width)
            || u64::from(y) + u64::from(h) > u64::from(height)
        {
            return Ok(None);
        }
        if w == 0 || h == 0 {
            return Ok(Some(Vec::new()));
        }
        let image = match &self.repr {
            Repr::Tiled(plane) => return plane.read_rect_with_job(x, y, w, h, job),
            Repr::Contiguous(image) => image,
        };
        let mut out = Vec::new();
        if out.try_reserve_exact(w as usize * h as usize).is_err() {
            return Ok(None);
        }
        let raw = image.as_raw();
        let stride = width as usize * 4;
        for row in y as usize..y as usize + h as usize {
            job.check()?;
            let start = row * stride + x as usize * 4;
            let line = &raw[start..start + w as usize * 4];
            out.extend(line.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]));
        }
        Ok(Some(out))
    }

    /// Row `y`, columns `x0..x1` (exclusive), as contiguous runs `(x,
    /// samples)`, left to right and adjoining. Both representations obey
    /// `TilePlane::row_segments`'s rules verbatim (empty for `y >= height` or
    /// `x0 >= x1`; `x1` clamped to `width`). Tiled: the store's runs, one per
    /// tile column. Contiguous: ONE run `(x0, &samples[y·w + x0 .. y·w +
    /// min(x1, w)])`. No allocation, no view.
    pub fn row_segments(&self, y: u32, x0: u32, x1: u32) -> LayerRowSegments<'_> {
        let inner = match &self.repr {
            Repr::Tiled(plane) => Segments::Tiled(plane.row_segments(y, x0, x1)),
            Repr::Contiguous(image) => {
                let (width, height) = image.dimensions();
                let x1 = x1.min(width);
                let run = (y < height && x0 < x1).then(|| {
                    let row = y as usize * width as usize;
                    (x0, &samples(image)[row + x0 as usize..row + x1 as usize])
                });
                Segments::Contiguous(run)
            }
        };
        LayerRowSegments { inner }
    }

    /// Every row's runs in plane order (row-major, left to right):
    /// `row_segments(y, 0, width)` for `y` in `0..height`. No allocation, no
    /// view.
    pub fn runs(&self) -> impl Iterator<Item = &[[u8; 4]]> + '_ {
        let (width, height) = self.dimensions();
        (0..height).flat_map(move |y| self.row_segments(y, 0, width).map(|(_, run)| run))
    }

    /// The sample at `(x, y)`; `None` outside the raster. No allocation, no
    /// view.
    pub fn sample_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        match &self.repr {
            Repr::Tiled(plane) => plane.sample_at(x, y),
            Repr::Contiguous(image) => {
                let (width, height) = image.dimensions();
                if x >= width || y >= height {
                    return None;
                }
                Some(samples(image)[y as usize * width as usize + x as usize])
            }
        }
    }

    /// Identity (the token, `Arc::ptr_eq`).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.identity, &b.identity)
    }

    /// Live weak handles onto this raster's identity (style-cache and
    /// content-box keys). Test oracle only.
    pub fn weak_count(&self) -> usize {
        Arc::weak_count(&self.identity)
    }

    /// The identity key the style cache and the content-box memo hold.
    pub(crate) fn key(&self) -> LayerRasterKey {
        LayerRasterKey {
            image: Arc::downgrade(&self.identity),
        }
    }

    /// The address of the identity token, as an opaque storage identity
    /// (`dr_doc_layer_storage_identity`). Two rasters answer equal values
    /// exactly when [`Self::ptr_eq`] holds, for as long as either is alive.
    pub(crate) fn storage_identity(&self) -> u64 {
        Arc::as_ptr(&self.identity) as usize as u64
    }

    /// Tiled: the index as `tile_structure` and every tile under `category`
    /// (a tile shared by several rasters counts once). Contiguous: the image
    /// structure (`shared_buffer_structure`) and its sample storage under
    /// `category`, 13B-2a's two records.
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

    fn tiled(plane: TilePlane<[u8; 4]>) -> Self {
        Self {
            repr: Repr::Tiled(plane),
            identity: Arc::new(()),
        }
    }

    fn contiguous(image: Arc<RgbaImage>) -> Self {
        Self {
            repr: Repr::Contiguous(image),
            identity: Arc::new(()),
        }
    }
}

/// The runs of one row span (see [`LayerRaster::row_segments`]).
pub struct LayerRowSegments<'a> {
    inner: Segments<'a>,
}

enum Segments<'a> {
    Tiled(RowSegments<'a, [u8; 4]>),
    Contiguous(Option<(u32, &'a [[u8; 4]])>),
}

impl<'a> Iterator for LayerRowSegments<'a> {
    type Item = (u32, &'a [[u8; 4]]);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            Segments::Tiled(runs) => runs.next(),
            Segments::Contiguous(run) => run.take(),
        }
    }
}

/// A non-owning identity handle onto a [`LayerRaster`].
#[derive(Clone)]
pub(crate) struct LayerRasterKey {
    image: Weak<()>,
}

impl LayerRasterKey {
    /// Same identity (`std::ptr::eq` on the weak pointers).
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::ptr::eq(self.image.as_ptr(), other.image.as_ptr())
    }

    /// The raster this key named has been dropped.
    pub(crate) fn is_dead(&self) -> bool {
        self.image.strong_count() == 0
    }
}

/// Caches `image` as `plane`'s view (the writer already holds it).
fn seed(plane: &TilePlane<[u8; 4]>, image: RgbaImage) {
    contiguous_views::seed(
        PlaneKey::Rgba8(plane.weak_index()),
        View::Rgba8(Arc::new(image)),
    );
}

/// The plane's cached view, materializing it outside the lock on a miss.
fn view(plane: &TilePlane<[u8; 4]>) -> Arc<RgbaImage> {
    contiguous_views::view(PlaneKey::Rgba8(plane.weak_index()), || {
        View::Rgba8(Arc::new(
            materialize_with_job(plane, JobContext::NONE).expect(ALLOCATION),
        ))
    })
    .into_rgba8()
    .expect("a byte plane's view is an RGBA8 image")
}

fn materialize_with_job(
    plane: &TilePlane<[u8; 4]>,
    job: JobContext<'_>,
) -> Result<RgbaImage, String> {
    let (width, height) = plane.dimensions();
    let samples = plane.to_contiguous_with_job(job).map_err(|e| {
        if job.cancelled() {
            e
        } else {
            ALLOCATION.to_string()
        }
    })?;
    RgbaImage::from_raw(width, height, samples.into_flattened()).ok_or_else(|| ALLOCATION.into())
}
