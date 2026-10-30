//! The shared immutable TILE STORE (Phase 13B-1): one plane of samples split
//! into fixed 256×256 tiles, each tile an `Arc`-shared immutable allocation,
//! for every retained sample type (u8/u16 gray, RGBA8/16/f32, CMYKA16). 13B-2b's
//! `LayerRaster` (byte `Layer.pixels`) holds `TilePlane<[u8; 4]>` tiles; the
//! store itself changes no pixel output or `.rz` byte stream.
//!
//! **Geometry.** Tile `(tx, ty)` covers `[tx·256, min(tx·256 + 256, w)) ×
//! [ty·256, min(ty·256 + 256, h))`; `columns = ⌈w/256⌉`, `rows = ⌈h/256⌉`,
//! the index is row-major (`ty·columns + tx`). Edge tiles are CLIPPED, never
//! padded: a 1×1 plane is one 1-sample tile, `to_contiguous` is exact and the
//! accounting carries no padding bytes. 256 keeps a tile at 256 KiB RGBA8 /
//! 512 KiB RGBA16 / 1 MiB RGBA32F, so a 24 MP layer is 384 tiles and a
//! 300×300 brush edit touches at most 4 of them.
//!
//! **Purity and the bitwise sharing latch.** A plane is never mutated.
//! `with_rect` / `rebuilt_from` compose each visited tile's new contents and
//! keep the OLD `Arc` when every sample is [`Sample::same_bits`] with the old
//! one; only a changed tile is allocated, every other tile is shared and a
//! fresh index is minted. The latch is bitwise on purpose: an f32 `-0.0`
//! written over `0.0` is stored-state movement and must allocate (the
//! `geometry_noops.rs` rule), and a NaN pattern written over itself is no
//! movement — `PartialEq` would judge both wrongly. These edits return the
//! three outcomes `Result<Option<Self>, String>`: `Ok(None)` is the purity
//! rule's "nothing changed" (an identical copy would register a phantom undo
//! step), `Err` is cancellation, allocation or a rect leaving the plane.
//! `shared_tile_count` and [`accounting`] are pointer facts and never compare
//! samples. `same_bits` (plane to plane) and `equals_contiguous` (plane to a
//! row-major slice) are the ONLY tile equality: a pointer short-circuit per
//! tile, then `Sample::same_bits` per sample. `from_rows_shared_with_job`
//! builds a plane row by row as `from_rows_with_job` does and
//! shares ONE `Arc` among its finished bitwise-uniform tiles of each
//! `((w, h), value)`; `from_rows_with_job` keeps every tile fresh.
//!
//! **Cancellation.** The `_with_job` variants poll `job.check()` once per
//! VISITED tile before its copy/compare (checkpoints only, no reports):
//! `with_rect` and `read_rect` visit the tiles their rect intersects,
//! the other three visit every tile. The public wrappers begin the control (one checkpoint), run
//! the variant and report `(1.0, 5)` (one more), so an uncancelled run ends at
//! `visited + 2` checkpoints. Declared cancel-to-return bound: `T_f + c_tile`
//! (the return floor plus one tile's copy/compare).
//!
//! **Accounting.** `record_resources` records the index as `tile_structure`
//! and each tile under a CALLER-SUPPLIED category, keyed by the tile's sample
//! pointer through `Inventory`, so a tile shared by several planes (undo
//! states, snapshots) counts once and 13B-2 keeps its tiles under
//! `layer_pixels` without changing a report key.
//!
//! **Reads in place (13B-2c-1).** `row_segments` answers one row span as
//! the contiguous runs the tiles already hold (one borrowed slice per tile
//! column, left to right, adjoining) and `sample_at` one sample; neither
//! allocates, so the compositor and the regional per-sample tree read a
//! tiled plane without materializing a contiguous copy. `read_rect` stays the
//! copying window read.
//!
//! **The 13C seam.** `Tile<S>` is opaque behind `samples()`, so a spilled or
//! compressed tile variant can land later without changing `TilePlane`.
//!
//! `TilePlane<S>` over the six retained sample types (u8/u16 gray, RGBA8/16/f32, CMYKA16), fixed 256×256
//! tiles, row-major index, edge tiles CLIPPED (no padding; `to_contiguous` exact). `with_rect` /
//! `rebuilt_from` keep an old tile's `Arc` when every sample is `Sample::same_bits` (BITWISE: f32
//! `-0.0` over `0.0` allocates, a NaN pattern over itself does not), allocate only changed tiles,
//! mint a fresh index and answer `Ok(None)` when nothing changed. `weak_index` is the plane's index
//! handle (the `layer_raster` view-cache key). Its first consumer is 13B-2b's `LayerRaster`. Since
//! 13B-2c-1 `row_segments(y, x0, x1)` answers a row span as the tiles' own borrowed runs (one per
//! tile column, left to right, adjoining; empty for `y >= height` or `x0 >= x1`, `x1` clamped to
//! `width`) and `sample_at` one sample (`None` outside) — both allocation-free, `read_rect`'s
//! slicing arithmetic. Since 13B-2c-2b `same_bits(&other)` (plane to plane) and
//! `equals_contiguous(&[S])` (plane to a row-major slice of exactly `w·h`) are the ONLY tile
//! equality — a pointer short-circuit per tile, then `Sample::same_bits` per sample,
//! `rebuilt_from_with_job`'s slicing; no allocation, no job.

use std::collections::HashSet;
use std::mem::size_of;
use std::sync::{Arc, Weak};

use crate::doc::MAX_PIXELS;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::reserved_vec;
use crate::resources::Inventory;

/// Tile edge in pixels (see the module doc for why 256).
pub const TILE_EDGE: u32 = 256;

const EDGE: usize = TILE_EDGE as usize;

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for [u8; 4] {}
    impl Sealed for [u16; 4] {}
    impl Sealed for [f32; 4] {}
    impl Sealed for [u16; 5] {}
}

/// One retained sample type. Sealed: exactly the six storages documents keep
/// (the sixth, straight CMYKA16, is the CMYK document's, `cmyk_image`).
pub trait Sample: Copy + Default + Send + Sync + 'static + sealed::Sealed {
    /// Stored-state equality: integer types `==`; `[f32; 4]` compares
    /// `f32::to_bits` per component (so -0.0 ≠ 0.0 and a NaN pattern equals
    /// itself). The ONLY equality the sharing latch may use.
    fn same_bits(self, other: Self) -> bool;
}

impl Sample for u8 {
    fn same_bits(self, other: Self) -> bool {
        self == other
    }
}
impl Sample for u16 {
    fn same_bits(self, other: Self) -> bool {
        self == other
    }
}
impl Sample for [u8; 4] {
    fn same_bits(self, other: Self) -> bool {
        self == other
    }
}
impl Sample for [u16; 4] {
    fn same_bits(self, other: Self) -> bool {
        self == other
    }
}
impl Sample for [u16; 5] {
    fn same_bits(self, other: Self) -> bool {
        self == other
    }
}
impl Sample for [f32; 4] {
    fn same_bits(self, other: Self) -> bool {
        self.iter()
            .zip(other.iter())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

/// One immutable tile: row-major samples, stride = width, no padding.
pub struct Tile<S: Sample> {
    width: u32,
    height: u32,
    samples: Box<[S]>,
}

impl<S: Sample> Tile<S> {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    /// Row-major, stride = `width`, no padding.
    pub fn samples(&self) -> &[S] {
        &self.samples
    }
    /// Sample payload bytes (the retained allocation's size).
    pub fn bytes(&self) -> u64 {
        (self.samples.len() * size_of::<S>()) as u64
    }
}

/// An immutable tiled plane; cloning shares the index and every tile.
#[derive(Clone)]
pub struct TilePlane<S: Sample> {
    width: u32,
    height: u32,
    tiles: Arc<[Arc<Tile<S>>]>,
}

/// Pointer-deduplicated totals over a set of planes (see [`accounting`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileAccounting {
    pub unique_tiles: u64,
    pub referenced_tiles: u64,
    pub unique_bytes: u64,
    pub referenced_bytes: u64,
    pub index_bytes: u64,
}

/// Tiles unique by their sample allocation's pointer (as `Inventory::record`
/// keys them); indices unique by the index allocation's pointer. Pointer
/// facts only: equal contents in distinct allocations count twice.
pub fn accounting<S: Sample>(planes: &[&TilePlane<S>]) -> TileAccounting {
    let mut out = TileAccounting::default();
    let mut tiles = HashSet::new();
    let mut indices = HashSet::new();
    for plane in planes {
        if indices.insert(plane.tiles.as_ptr() as usize) {
            out.index_bytes += plane.index_bytes();
        }
        for tile in plane.tiles.iter() {
            out.referenced_tiles += 1;
            out.referenced_bytes += tile.bytes();
            if tiles.insert(tile.samples.as_ptr() as usize) {
                out.unique_tiles += 1;
                out.unique_bytes += tile.bytes();
            }
        }
    }
    out
}

fn sample_count(width: u32, height: u32) -> Result<usize, String> {
    let count = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || count > MAX_PIXELS {
        return Err("tile plane dimensions exceed the supported pixel limit".into());
    }
    Ok(count as usize)
}

fn mismatch() -> String {
    "tile plane sample count does not match dimensions".into()
}

fn allocate<T>(count: usize) -> Result<Vec<T>, String> {
    reserved_vec(count).map_err(|_| "could not allocate tile storage".to_string())
}

/// Copies the `w × h` window at `(x, y)` of a row-major source with `stride`.
fn tile_from<S: Sample>(
    source: &[S],
    stride: usize,
    (x, y, w, h): (usize, usize, usize, usize),
) -> Result<Tile<S>, String> {
    let mut samples = allocate(w * h)?;
    for row in y..y + h {
        let at = row * stride + x;
        samples.extend_from_slice(&source[at..at + w]);
    }
    Ok(Tile {
        width: w as u32,
        height: h as u32,
        samples: samples.into_boxed_slice(),
    })
}

/// A tile's `(width, height)`: `filled` shares one allocation per shape.
type TileShape = (u32, u32);

fn index_from<S: Sample>(tiles: Vec<Arc<Tile<S>>>) -> Arc<[Arc<Tile<S>>]> {
    Arc::from(tiles)
}

impl<S: Sample> TilePlane<S> {
    /// A plane holding `value` everywhere. Every full interior tile shares one
    /// allocation, as do the right-edge column, the bottom-edge row and the
    /// corner: at most 4 tile allocations.
    pub fn filled(width: u32, height: u32, value: S) -> Result<Self, String> {
        sample_count(width, height)?;
        let (columns, rows) = (width.div_ceil(TILE_EDGE), height.div_ceil(TILE_EDGE));
        let mut shapes: Vec<(TileShape, Arc<Tile<S>>)> = Vec::with_capacity(4);
        let mut tiles = allocate((columns * rows) as usize)?;
        for ty in 0..rows {
            for tx in 0..columns {
                let (_, _, w, h) = clipped(width, height, tx, ty);
                let tile = match shapes.iter().find(|(shape, _)| *shape == (w, h)) {
                    Some((_, tile)) => Arc::clone(tile),
                    None => {
                        let n = (w * h) as usize;
                        let mut samples = allocate(n)?;
                        samples.resize(n, value);
                        let tile = Arc::new(Tile {
                            width: w,
                            height: h,
                            samples: samples.into_boxed_slice(),
                        });
                        shapes.push(((w, h), Arc::clone(&tile)));
                        tile
                    }
                };
                tiles.push(tile);
            }
        }
        Ok(Self {
            width,
            height,
            tiles: index_from(tiles),
        })
    }

    /// Tiles a row-major contiguous plane (`samples.len() == w·h`).
    pub fn from_contiguous(
        width: u32,
        height: u32,
        samples: &[S],
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = Self::from_contiguous_with_job(width, height, samples, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The exact row-major contiguous plane.
    pub fn to_contiguous(&self, control: Option<&DrJobControl>) -> Result<Vec<S>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = self.to_contiguous_with_job(job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The row-major samples of a rect; `None` when it leaves the plane (or
    /// its copy cannot be allocated). A zero-area rect is `Some(vec![])`.
    pub fn read_rect(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Vec<S>> {
        self.read_rect_with_job(x, y, w, h, JobContext::NONE)
            .ok()
            .flatten()
    }

    /// [`Self::read_rect`] polling `job` once per visited tile, on the
    /// rect's first row inside each tile band, before that tile's span is
    /// copied — `with_rect_with_job`'s count. `Ok(None)` is `read_rect`'s
    /// `None`; `Err` is the control's cancel sentence.
    pub(crate) fn read_rect_with_job(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        job: JobContext<'_>,
    ) -> Result<Option<Vec<S>>, String> {
        if w == 0 || h == 0 {
            return Ok(Some(Vec::new()));
        }
        if !self.contains(x, y, w, h) {
            return Ok(None);
        }
        let Ok(mut out) = allocate((w as usize) * (h as usize)) else {
            return Ok(None);
        };
        let (x0, x1) = (x as usize, (x + w) as usize);
        for row in y as usize..(y + h) as usize {
            let ty = row / EDGE;
            let band_start = row == y as usize || row % EDGE == 0;
            for tx in x0 / EDGE..=(x1 - 1) / EDGE {
                if band_start {
                    job.check()?;
                }
                let tile = self.tile_at(tx, ty);
                let (tx0, tw) = (tx * EDGE, tile.width as usize);
                let (a, b) = (x0.max(tx0), x1.min(tx0 + tw));
                let at = (row - ty * EDGE) * tw;
                out.extend_from_slice(&tile.samples[at + a - tx0..at + b - tx0]);
            }
        }
        Ok(Some(out))
    }

    /// The plane with the rect overwritten by `samples` (row-major, stride
    /// `w`). `Ok(None)` when no tile changed bitwise (or the rect is empty).
    pub fn with_rect(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[S],
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = self.with_rect_with_job(x, y, w, h, samples, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The plane rebuilt from a whole contiguous plane of the same
    /// dimensions, sharing every bitwise-unchanged tile. `Ok(None)` when no
    /// tile changed.
    pub fn rebuilt_from(
        &self,
        samples: &[S],
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let out = self.rebuilt_from_with_job(samples, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Row `y`, columns `x0..x1` (exclusive), as the contiguous runs the
    /// tiles hold: `(x, samples)` per tile column, left to right, `x` = the
    /// run's first column, runs adjoining (`x_next == x + len`). Empty when
    /// `y >= height` or `x0 >= x1`; `x1` is clamped to `width`. No
    /// allocation.
    pub fn row_segments(&self, y: u32, x0: u32, x1: u32) -> RowSegments<'_, S> {
        let x1 = x1.min(self.width);
        let (x, x1) = if y >= self.height || x0 >= x1 {
            (0, 0)
        } else {
            (x0 as usize, x1 as usize)
        };
        RowSegments {
            plane: self,
            y: y as usize,
            x,
            x1,
        }
    }

    /// The sample at `(x, y)`; `None` outside the plane. No allocation.
    pub fn sample_at(&self, x: u32, y: u32) -> Option<S> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        let tile = self.tile_at(x / EDGE, y / EDGE);
        Some(tile.samples()[(y % EDGE) * tile.width as usize + x % EDGE])
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn columns(&self) -> u32 {
        self.width.div_ceil(TILE_EDGE)
    }
    pub fn rows(&self) -> u32 {
        self.height.div_ceil(TILE_EDGE)
    }

    pub fn tile(&self, tx: u32, ty: u32) -> Option<&Arc<Tile<S>>> {
        if tx >= self.columns() || ty >= self.rows() {
            return None;
        }
        Some(self.tile_at(tx as usize, ty as usize))
    }

    /// `(x, y, w, h)` of tile `(tx, ty)`, edge tiles clipped.
    pub fn tile_rect(&self, tx: u32, ty: u32) -> Option<(u32, u32, u32, u32)> {
        if tx >= self.columns() || ty >= self.rows() {
            return None;
        }
        Some(clipped(self.width, self.height, tx, ty))
    }

    /// Tiles shared by pointer at the same index; 0 when dimensions differ.
    pub fn shared_tile_count(&self, other: &Self) -> usize {
        if self.dimensions() != other.dimensions() {
            return 0;
        }
        self.tiles
            .iter()
            .zip(other.tiles.iter())
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count()
    }

    /// Bitwise sample equality with `other`: `false` when dimensions differ;
    /// else every tile index is the same `Arc` or [`Sample::same_bits`]
    /// sample for sample. No allocation, no job.
    pub fn same_bits(&self, other: &Self) -> bool {
        if self.dimensions() != other.dimensions() {
            return false;
        }
        self.tiles.iter().zip(other.tiles.iter()).all(|(a, b)| {
            Arc::ptr_eq(a, b)
                || a.samples()
                    .iter()
                    .zip(b.samples())
                    .all(|(x, y)| x.same_bits(*y))
        })
    }

    /// Bitwise sample equality with the row-major plane `samples` (stride
    /// `w`): `false` unless `samples.len() == w·h`; else each tile's rows are
    /// [`Sample::same_bits`] against `samples[(y + r)·w + x ..]` (the
    /// `rebuilt_from_with_job` slicing). No allocation, no job.
    pub fn equals_contiguous(&self, samples: &[S]) -> bool {
        let stride = self.width as usize;
        if samples.len() != stride * self.height as usize {
            return false;
        }
        let columns = self.columns() as usize;
        (0..self.rows() as usize).all(|ty| {
            (0..columns).all(|tx| {
                let tile = self.tile_at(tx, ty);
                let (x, y, w) = (tx * EDGE, ty * EDGE, tile.width as usize);
                tile.samples.chunks_exact(w).enumerate().all(|(r, row)| {
                    let at = (y + r) * stride + x;
                    row.iter()
                        .zip(&samples[at..at + w])
                        .all(|(a, b)| a.same_bits(*b))
                })
            })
        })
    }

    /// Unique retained bytes of this plane alone (tiles plus index).
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory, "tile_pixels", false);
        inventory.unique_bytes()
    }

    pub(crate) fn from_contiguous_with_job(
        width: u32,
        height: u32,
        samples: &[S],
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        if samples.len() != sample_count(width, height)? {
            return Err(mismatch());
        }
        let (columns, rows) = (width.div_ceil(TILE_EDGE), height.div_ceil(TILE_EDGE));
        let mut tiles = allocate((columns * rows) as usize)?;
        for ty in 0..rows {
            for tx in 0..columns {
                job.check()?;
                let (x, y, w, h) = clipped(width, height, tx, ty);
                let rect = (x as usize, y as usize, w as usize, h as usize);
                tiles.push(Arc::new(tile_from(samples, width as usize, rect)?));
            }
        }
        Ok(Self {
            width,
            height,
            tiles: index_from(tiles),
        })
    }

    /// Builds the plane one 256-row tile band at a time: `fill_row(y, row)`
    /// writes row `y` (`row.len() == width`). Holds one row buffer plus the
    /// band's own (final) tiles; `job.check()` once per band. The same
    /// refusals as [`Self::from_contiguous`] (the banded native reopen
    /// decodes PNG rows straight into tiles through it).
    pub(crate) fn from_rows_with_job(
        width: u32,
        height: u32,
        fill_row: impl FnMut(u32, &mut [S]) -> Result<(), String>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Self::from_rows_body(width, height, fill_row, false, job)
    }

    /// [`Self::from_rows_with_job`]'s statements and polls, then every
    /// finished tile whose samples are all [`Sample::same_bits`] to its first
    /// sample shares ONE `Arc` per `((w, h), value)`: the first such tile's
    /// is kept and a later duplicate is dropped as its band finishes (the
    /// uniform-source resample's output).
    pub(crate) fn from_rows_shared_with_job(
        width: u32,
        height: u32,
        fill_row: impl FnMut(u32, &mut [S]) -> Result<(), String>,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        Self::from_rows_body(width, height, fill_row, true, job)
    }

    fn from_rows_body(
        width: u32,
        height: u32,
        mut fill_row: impl FnMut(u32, &mut [S]) -> Result<(), String>,
        share: bool,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        sample_count(width, height)?;
        let mut uniform: Vec<Arc<Tile<S>>> = Vec::new();
        let (columns, rows) = (width.div_ceil(TILE_EDGE), height.div_ceil(TILE_EDGE));
        let mut tiles = allocate((columns * rows) as usize)?;
        let mut row = allocate(width as usize)?;
        row.resize(width as usize, S::default());
        for ty in 0..rows {
            job.check()?;
            let mut band: Vec<(u32, u32, Vec<S>)> = allocate(columns as usize)?;
            for tx in 0..columns {
                let (_, _, w, h) = clipped(width, height, tx, ty);
                band.push((w, h, allocate((w * h) as usize)?));
            }
            let (_, y, _, h) = clipped(width, height, 0, ty);
            for r in y..y + h {
                fill_row(r, &mut row)?;
                for (tx, (w, _, samples)) in band.iter_mut().enumerate() {
                    let x = tx * EDGE;
                    samples.extend_from_slice(&row[x..x + *w as usize]);
                }
            }
            for (w, h, samples) in band {
                let first = samples[0];
                let uniform_tile = share && samples.iter().all(|s| s.same_bits(first));
                if uniform_tile {
                    if let Some(tile) = uniform.iter().find(|tile| {
                        (tile.width, tile.height) == (w, h) && tile.samples[0].same_bits(first)
                    }) {
                        tiles.push(Arc::clone(tile));
                        continue;
                    }
                }
                let tile = Arc::new(Tile {
                    width: w,
                    height: h,
                    samples: samples.into_boxed_slice(),
                });
                if uniform_tile {
                    uniform.push(Arc::clone(&tile));
                }
                tiles.push(tile);
            }
        }
        Ok(Self {
            width,
            height,
            tiles: index_from(tiles),
        })
    }

    /// The one sample every tile holds when the plane is bitwise uniform
    /// ([`Sample::same_bits`]), else `None`. Allocates nothing.
    pub(crate) fn uniform_sample(&self) -> Option<S> {
        let first = *self.tiles.first()?.samples.first()?;
        let mut previous: Option<&Arc<Tile<S>>> = None;
        for tile in self.tiles.iter() {
            // A tile shared with the one just checked holds the same samples
            // (`filled`, a shared output): skipping it answers the same.
            if previous.is_some_and(|p| Arc::ptr_eq(p, tile)) {
                continue;
            }
            if !tile.samples.iter().all(|s| s.same_bits(first)) {
                return None;
            }
            previous = Some(tile);
        }
        Some(first)
    }

    pub(crate) fn to_contiguous_with_job(&self, job: JobContext<'_>) -> Result<Vec<S>, String> {
        let stride = self.width as usize;
        let n = stride * self.height as usize;
        let mut out = allocate(n)?;
        out.resize(n, S::default());
        for ty in 0..self.rows() {
            for tx in 0..self.columns() {
                job.check()?;
                let tile = self.tile_at(tx as usize, ty as usize);
                let (x, y) = ((tx as usize) * EDGE, (ty as usize) * EDGE);
                let w = tile.width as usize;
                for (r, row) in tile.samples.chunks_exact(w).enumerate() {
                    let at = (y + r) * stride + x;
                    out[at..at + w].copy_from_slice(row);
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn with_rect_with_job(
        &self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        samples: &[S],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if samples.len() as u64 != u64::from(w) * u64::from(h) {
            return Err(mismatch());
        }
        if w == 0 || h == 0 {
            return Ok(None);
        }
        if !self.contains(x, y, w, h) {
            return Err("tile rect leaves the plane".into());
        }
        let (rx, ry, rw, rh) = (x as usize, y as usize, w as usize, h as usize);
        let mut index: Option<Vec<Arc<Tile<S>>>> = None;
        for ty in ry / EDGE..=(ry + rh - 1) / EDGE {
            for tx in rx / EDGE..=(rx + rw - 1) / EDGE {
                job.check()?;
                let old = self.tile_at(tx, ty);
                let (tx0, ty0, tw) = (tx * EDGE, ty * EDGE, old.width as usize);
                let (ax, bx) = (rx.max(tx0), (rx + rw).min(tx0 + tw));
                let (ay, by) = (ry.max(ty0), (ry + rh).min(ty0 + old.height as usize));
                let span = bx - ax;
                // Row slices of the intersection: (tile offset, source offset).
                let rows = (ay..by).map(|r| ((r - ty0) * tw + ax - tx0, (r - ry) * rw + ax - rx));
                let changed = rows.clone().any(|(t, s)| {
                    !old.samples[t..t + span]
                        .iter()
                        .zip(&samples[s..s + span])
                        .all(|(a, b)| a.same_bits(*b))
                });
                if !changed {
                    continue;
                }
                let mut copy = allocate(old.samples.len())?;
                copy.extend_from_slice(&old.samples);
                for (t, s) in rows {
                    copy[t..t + span].copy_from_slice(&samples[s..s + span]);
                }
                let tile = Arc::new(Tile {
                    width: old.width,
                    height: old.height,
                    samples: copy.into_boxed_slice(),
                });
                let columns = self.columns() as usize;
                self.edit_index(&mut index)?[ty * columns + tx] = tile;
            }
        }
        Ok(index.map(|tiles| self.with_index(tiles)))
    }

    pub(crate) fn rebuilt_from_with_job(
        &self,
        samples: &[S],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let stride = self.width as usize;
        if samples.len() != stride * self.height as usize {
            return Err(mismatch());
        }
        let mut index: Option<Vec<Arc<Tile<S>>>> = None;
        let columns = self.columns() as usize;
        for ty in 0..self.rows() as usize {
            for tx in 0..columns {
                job.check()?;
                let old = self.tile_at(tx, ty);
                let (x, y, w) = (tx * EDGE, ty * EDGE, old.width as usize);
                let changed = old.samples.chunks_exact(w).enumerate().any(|(r, row)| {
                    let at = (y + r) * stride + x;
                    !row.iter()
                        .zip(&samples[at..at + w])
                        .all(|(a, b)| a.same_bits(*b))
                });
                if !changed {
                    continue;
                }
                let tile = tile_from(samples, stride, (x, y, w, old.height as usize))?;
                self.edit_index(&mut index)?[ty * columns + tx] = Arc::new(tile);
            }
        }
        Ok(index.map(|tiles| self.with_index(tiles)))
    }

    /// The index as one `tile_structure` allocation, each tile under the
    /// caller's `category` (13B-2 keeps layer tiles under `layer_pixels`).
    pub(crate) fn record_resources(
        &self,
        inventory: &mut Inventory,
        category: &'static str,
        cache: bool,
    ) {
        inventory.record(
            self.tiles.as_ptr() as usize,
            self.index_bytes() as usize,
            "tile_structure",
            cache,
        );
        for tile in self.tiles.iter() {
            inventory.record(
                tile.samples.as_ptr() as usize,
                tile.bytes() as usize,
                category,
                cache,
            );
        }
    }

    /// A non-owning handle onto this plane's index allocation (the key the
    /// contiguous-view cache in `layer_raster` holds).
    pub(crate) fn weak_index(&self) -> Weak<[Arc<Tile<S>>]> {
        Arc::downgrade(&self.tiles)
    }

    fn index_bytes(&self) -> u64 {
        (self.tiles.len() * size_of::<Arc<Tile<S>>>()) as u64
    }

    fn tile_at(&self, tx: usize, ty: usize) -> &Arc<Tile<S>> {
        &self.tiles[ty * self.columns() as usize + tx]
    }

    fn contains(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        u64::from(x) + u64::from(w) <= u64::from(self.width)
            && u64::from(y) + u64::from(h) <= u64::from(self.height)
    }

    /// The fresh index, minted (sharing every tile) on the first change.
    fn edit_index<'a>(
        &self,
        index: &'a mut Option<Vec<Arc<Tile<S>>>>,
    ) -> Result<&'a mut Vec<Arc<Tile<S>>>, String> {
        let tiles = match index.take() {
            Some(tiles) => tiles,
            None => {
                let mut tiles = allocate(self.tiles.len())?;
                tiles.extend(self.tiles.iter().cloned());
                tiles
            }
        };
        Ok(index.insert(tiles))
    }

    fn with_index(&self, tiles: Vec<Arc<Tile<S>>>) -> Self {
        Self {
            width: self.width,
            height: self.height,
            tiles: index_from(tiles),
        }
    }
}

/// The runs of one row span (see [`TilePlane::row_segments`]).
pub struct RowSegments<'a, S: Sample> {
    plane: &'a TilePlane<S>,
    y: usize,
    x: usize,
    x1: usize,
}

impl<'a, S: Sample> Iterator for RowSegments<'a, S> {
    type Item = (u32, &'a [S]);

    /// The next tile column's run, sliced exactly as `read_rect` slices a
    /// row: `[max(x, tx0), min(x1, tx0 + tw))` of the tile's row `y % EDGE`.
    fn next(&mut self) -> Option<Self::Item> {
        if self.x >= self.x1 {
            return None;
        }
        let plane: &'a TilePlane<S> = self.plane;
        let tx = self.x / EDGE;
        let tile = plane.tile_at(tx, self.y / EDGE);
        let (tx0, tw) = (tx * EDGE, tile.width as usize);
        let (a, b) = (self.x, self.x1.min(tx0 + tw));
        let at = (self.y % EDGE) * tw;
        self.x = b;
        Some((a as u32, &tile.samples[at + a - tx0..at + b - tx0]))
    }
}

fn clipped(width: u32, height: u32, tx: u32, ty: u32) -> (u32, u32, u32, u32) {
    let (x, y) = (tx * TILE_EDGE, ty * TILE_EDGE);
    (
        x,
        y,
        (width - x).min(TILE_EDGE),
        (height - y).min(TILE_EDGE),
    )
}
