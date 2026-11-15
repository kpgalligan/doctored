//! The Magnetic Lasso's per-capture edge map ([`DrEdgeMap`],
//! `dr_edge_map_create`): the edge samples (`edge_trace_cost::gradients`) of
//! one image, computed in 16×16 tiles on first touch and kept, so that a
//! host session tracing the same capture on every pointer move computes each
//! sample once rather than once per move.
//!
//! A map answers for one image only: it records the dimensions and the
//! address of the pixel buffer it was made from and refuses any other
//! ("edge map was made for another image"). A `DrImage`'s pixels never
//! change after it is made, so an address with matching dimensions is that
//! image while the host keeps it alive. It holds at most [`MAP_CAP`] samples;
//! a tile that would pass that drops every tile first. A trace copies every
//! sample it uses out of the map before it costs anything, so what the map
//! holds or drops can never change a result: a trace is the same with any map
//! of its image or with a fresh one.

use crate::edge_trace::REFUSAL_CAP;
use crate::edge_trace_cost::{gradients, Gradient};
use image::RgbaImage;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// A tile's side in pixels.
const TILE: i64 = 16;
/// Samples per tile.
const TILE_SAMPLES: usize = (TILE * TILE) as usize;
/// The most samples a map holds: eight refusal caps (8,388,608 samples,
/// 64 MB), room for the tiles of several largest corridors at once while a
/// session can never grow without bound.
pub(crate) const MAP_CAP: usize = 8 * REFUSAL_CAP as usize;
/// A tile not computed yet.
const EMPTY: u32 = u32::MAX;

/// The edge samples of one image, behind a lock so any thread may trace.
pub struct DrEdgeMap {
    tiles: Mutex<Tiles>,
}

struct Tiles {
    width: u32,
    height: u32,
    /// The address of the image's pixel buffer.
    pixels: usize,
    cap: usize,
    /// Tiles per row of tiles.
    across: usize,
    /// Per tile (raster order), its slot in `samples`, or [`EMPTY`].
    slots: Vec<u32>,
    /// [`TILE_SAMPLES`] per slot, row-major within the tile; a tile cut by
    /// the image's right or bottom edge leaves the rest of its slot unused.
    samples: Vec<Gradient>,
}

impl DrEdgeMap {
    /// An empty map of `image` holding at most [`MAP_CAP`] samples.
    pub(crate) fn new(image: &RgbaImage) -> Self {
        Self::with_cap(image, MAP_CAP)
    }

    /// An empty map of `image` holding at most `cap` samples (at least one
    /// tile's worth whatever `cap` says).
    pub(crate) fn with_cap(image: &RgbaImage, cap: usize) -> Self {
        let (width, height) = image.dimensions();
        let tiles_of = |n: u32| (i64::from(n) + TILE - 1) as usize / TILE as usize;
        let across = tiles_of(width);
        DrEdgeMap {
            tiles: Mutex::new(Tiles {
                width,
                height,
                pixels: image.as_raw().as_ptr() as usize,
                cap,
                across,
                slots: vec![EMPTY; across * tiles_of(height)],
                samples: Vec::new(),
            }),
        }
    }

    /// Locks the map for one trace of `image`; another image refuses.
    pub(crate) fn samples<'a>(&'a self, image: &'a RgbaImage) -> Result<EdgeSamples<'a>, String> {
        // A panic inside a fill (caught at the FFI boundary) leaves the lock
        // poisoned but the map consistent: a tile's slot is recorded only
        // after its samples are complete, so the map is used as it stands.
        let tiles = self.tiles.lock().unwrap_or_else(PoisonError::into_inner);
        if image.dimensions() != (tiles.width, tiles.height)
            || image.as_raw().as_ptr() as usize != tiles.pixels
        {
            return Err("edge map was made for another image".into());
        }
        Ok(EdgeSamples {
            tiles,
            raw: image.as_raw(),
        })
    }

    /// The samples the map holds now, tiles' unused tails included.
    pub(crate) fn cached(&self) -> usize {
        self.tiles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .samples
            .len()
    }
}

/// A locked map and the pixels it samples.
pub(crate) struct EdgeSamples<'a> {
    tiles: MutexGuard<'a, Tiles>,
    raw: &'a [u8],
}

impl EdgeSamples<'_> {
    /// The edge sample of pixel `(x, y)`, its coordinates clamped to the
    /// image, computing its tile first if the map lacks it.
    pub(crate) fn gradient(&mut self, x: i64, y: i64) -> Gradient {
        let tiles = &mut *self.tiles;
        let (w, h) = (i64::from(tiles.width), i64::from(tiles.height));
        let (x, y) = (x.clamp(0, w - 1), y.clamp(0, h - 1));
        let tile = (y / TILE) as usize * tiles.across + (x / TILE) as usize;
        let mut slot = tiles.slots[tile];
        if slot == EMPTY {
            slot = tiles.fill(self.raw, tile);
        }
        tiles.samples[slot as usize * TILE_SAMPLES + ((y % TILE) * TILE + x % TILE) as usize]
    }
}

impl Tiles {
    /// Computes `tile` into a new slot, first dropping every tile when one
    /// more would pass the cap.
    fn fill(&mut self, raw: &[u8], tile: usize) -> u32 {
        if !self.samples.is_empty() && self.samples.len() + TILE_SAMPLES > self.cap {
            self.slots.fill(EMPTY);
            self.samples.clear();
        }
        let slot = self.samples.len() / TILE_SAMPLES;
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let (x0, y0) = (
            (tile % self.across) as i64 * TILE,
            (tile / self.across) as i64 * TILE,
        );
        let run = (w - x0).min(TILE) as usize;
        self.samples
            .resize(self.samples.len() + TILE_SAMPLES, [0.0; 2]);
        for row in 0..(h - y0).min(TILE) {
            let start = slot * TILE_SAMPLES + (row * TILE) as usize;
            gradients(
                raw,
                w,
                h,
                x0,
                y0 + row,
                &mut self.samples[start..start + run],
            );
        }
        // Recorded last, so an interrupted fill leaves the tile empty.
        let slot = slot as u32;
        self.slots[tile] = slot;
        slot
    }
}
