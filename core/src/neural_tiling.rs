//! The tiling kernel of Filter ▸ Neural Filters (M4): Super Zoom and
//! JPEG Artifacts Removal run an image model on fixed-size square tiles, and
//! this module owns the grid, each tile's model input, the classical
//! resample under (or instead of) the model and the stitch.
//!
//! **The grid.** A tile is `tile` source pixels square: a CORE of `core =
//! tile − 2·pad` pixels plus `pad` pixels of context on every side. Cores step
//! by `core` from (0, 0), row-major, so they partition the image (the last
//! column and row are clipped); a tile reads from its core's origin less
//! `pad`, which may lie outside the image, and every read is clamped to the
//! image (edge replicate). Only the core of a tile is ever written, so the
//! output never contains a pixel the model saw within `pad` of a tile edge:
//! that is what makes the stitch seamless for a model whose receptive field
//! fits in the pad, and exact for a local one (a test's stub). Both `pad` and
//! `core` are multiples of 8, so every tile's read origin sits on the image's
//! 8×8 JPEG block grid and a model that strips JPEG blocking (FBCNN) sees the
//! blocks where the file put them.
//!
//! **Scales.** The model answers `model_scale` (1 or 4) output cells per source
//! pixel; the filter writes `scale` (1, 2 or 4), which must divide it. Each
//! output pixel's RGB is the mean of the `k × k` model cells it covers, `k =
//! model_scale / scale` (a box reduction: ×2 from a ×4 model is the mean of a
//! 2×2 block), clamped to 0…1 and rounded once. Alpha is never the model's: it
//! is always the classical Catmull-Rom sample (`classical_at`), so a
//! transparent edge zooms as the image's own resample does and a scale-1 pass
//! keeps every alpha code exactly. Without the model (Super Zoom's Enhance
//! Image Details off) RGB is that classical sample too.
//!
//! **The classical sample** is `sampling`'s separable Catmull-Rom
//! (`Kernel::Bicubic.weights`, alpha-weighted through `accumulate_unorm` and
//! `straight_unorm`, the transform walk's arithmetic) at source position
//! `s = (o + 0.5)/scale` on each axis, with clamped taps where the transform
//! walk takes transparent ones: an upscale has no outside to fade into. At
//! scale 1 the weights are (0, 1, 0, 0), so it returns the pixel itself.
use crate::sampling::{accumulate_unorm, straight_unorm, Kernel, Unorm, MAX_TAPS};

/// The tile side range a grid accepts, in source pixels.
const TILE_RANGE: std::ops::RangeInclusive<u32> = 16..=1024;
/// `pad` and `core` alignment: the JPEG block (module doc).
const ALIGN: u32 = 8;

/// A tiling of a `width × height` source for a model of `model_scale`
/// written at `scale` (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileGrid {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) tile: u32,
    pub(crate) pad: u32,
    pub(crate) scale: u32,
    pub(crate) model_scale: u32,
}

/// One tile: where its `tile × tile` read starts (source pixels, possibly
/// outside the image) and its core `(x, y, w, h)` inside the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tile {
    pub(crate) read: (i64, i64),
    pub(crate) core: (u32, u32, u32, u32),
}

impl TileGrid {
    /// The grid, refusing an empty image, a tile outside 16…1024, a pad or
    /// core that is not a multiple of 8, a core under 8, a model scale other
    /// than 1 or 4, a scale other than 1, 2 or 4, and a scale that does not
    /// divide the model's.
    pub(crate) fn new(
        width: u32,
        height: u32,
        tile: u32,
        pad: u32,
        scale: u32,
        model_scale: u32,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("neural filters need a non-empty image".into());
        }
        if !TILE_RANGE.contains(&tile) {
            return Err("neural tile must be 16 to 1024 pixels".into());
        }
        if !pad.is_multiple_of(ALIGN) {
            return Err("neural tile pad must be a multiple of 8".into());
        }
        let core = pad
            .checked_mul(2)
            .and_then(|both| tile.checked_sub(both))
            .unwrap_or(0);
        if core < ALIGN || !core.is_multiple_of(ALIGN) {
            return Err(
                "neural tile core (tile − 2·pad) must be a multiple of 8, at least 8".into(),
            );
        }
        if !matches!(model_scale, 1 | 4) {
            return Err("neural model scale must be 1 or 4".into());
        }
        if !matches!(scale, 1 | 2 | 4) {
            return Err("neural zoom must be 1, 2 or 4".into());
        }
        if !model_scale.is_multiple_of(scale) {
            return Err("neural zoom must divide the model's scale".into());
        }
        Ok(Self {
            width,
            height,
            tile,
            pad,
            scale,
            model_scale,
        })
    }

    /// The core side, `tile − 2·pad`.
    pub(crate) fn core(&self) -> u32 {
        self.tile - 2 * self.pad
    }

    /// The written image's size, `scale ×` the source's.
    pub(crate) fn output(&self) -> (u64, u64) {
        (
            u64::from(self.width) * u64::from(self.scale),
            u64::from(self.height) * u64::from(self.scale),
        )
    }

    fn columns(&self) -> u64 {
        u64::from(self.width.div_ceil(self.core()))
    }

    fn rows(&self) -> u64 {
        u64::from(self.height.div_ceil(self.core()))
    }

    /// The number of tiles.
    pub(crate) fn count(&self) -> u64 {
        self.columns() * self.rows()
    }

    /// Tile `index` in row-major order, or `None` past the last.
    pub(crate) fn tile(&self, index: u64) -> Option<Tile> {
        if index >= self.count() {
            return None;
        }
        let core = self.core();
        let column = (index % self.columns()) as u32;
        let row = (index / self.columns()) as u32;
        let (x, y) = (column * core, row * core);
        Some(Tile {
            read: (
                i64::from(x) - i64::from(self.pad),
                i64::from(y) - i64::from(self.pad),
            ),
            core: (x, y, core.min(self.width - x), core.min(self.height - y)),
        })
    }

    /// The model input's length: three `tile²` planes.
    pub(crate) fn input_len(&self) -> usize {
        3 * self.tile as usize * self.tile as usize
    }

    /// The model output's length: three `(tile·model_scale)²` planes.
    pub(crate) fn plane_len(&self) -> usize {
        let side = self.tile as usize * self.model_scale as usize;
        3 * side * side
    }
}

/// The clamped source pixel at `(x, y)`.
fn clamped<T: Unorm>(src: &[[T; 4]], grid: &TileGrid, x: i64, y: i64) -> [T; 4] {
    let x = x.clamp(0, i64::from(grid.width) - 1) as usize;
    let y = y.clamp(0, i64::from(grid.height) - 1) as usize;
    src[y * grid.width as usize + x]
}

/// Writes `tile`'s model input into `out` (`grid.input_len()` floats):
/// channel-major straight RGB in 0…1, reads clamped to the image.
pub(crate) fn tile_input<T: Unorm>(src: &[[T; 4]], grid: &TileGrid, tile: &Tile, out: &mut [f32]) {
    let side = grid.tile as usize;
    let area = side * side;
    for y in 0..side {
        for x in 0..side {
            let pixel = clamped(src, grid, tile.read.0 + x as i64, tile.read.1 + y as i64);
            for c in 0..3 {
                out[c * area + y * side + x] = pixel[c].raw() / T::MAX;
            }
        }
    }
}

/// The classical sample of output pixel `(x, y)` (module doc).
pub(crate) fn classical_at<T: Unorm>(src: &[[T; 4]], grid: &TileGrid, x: u32, y: u32) -> [T; 4] {
    let kernel = Kernel::Bicubic;
    let scale = f64::from(grid.scale);
    let mut wx = [0.0_f32; MAX_TAPS];
    let mut wy = [0.0_f32; MAX_TAPS];
    let fx = kernel.weights((f64::from(x) + 0.5) / scale, &mut wx);
    let fy = kernel.weights((f64::from(y) + 0.5) / scale, &mut wy);
    let mut acc = [0.0_f32; 4];
    for (j, weight_y) in wy.iter().enumerate().take(kernel.taps()) {
        for (i, weight_x) in wx.iter().enumerate().take(kernel.taps()) {
            let pixel = clamped(src, grid, fx + i as i64, fy + j as i64);
            accumulate_unorm(&mut acc, &pixel, weight_y * weight_x);
        }
    }
    straight_unorm(acc)
}

/// Writes `tile`'s core, `scale ×` over, into the whole output `out`
/// (`grid.output()` pixels, row-major): alpha always `classical_at`; RGB the
/// mean of the `k × k` cells of the model's `plane` (`grid.plane_len()`
/// floats, channel-major) each pixel covers, at model coordinates
/// `(o − read·scale)·k + i`, clamped to 0…1 and rounded, or with no plane
/// `classical_at`'s RGB.
pub(crate) fn write_tile<T: Unorm>(
    src: &[[T; 4]],
    grid: &TileGrid,
    tile: &Tile,
    plane: Option<&[f32]>,
    out: &mut [[T; 4]],
) {
    let scale = grid.scale;
    let k = (grid.model_scale / scale) as usize;
    let side = grid.tile as usize * grid.model_scale as usize;
    let area = side * side;
    let cells = (k * k) as f32;
    let width = grid.output().0 as usize;
    let (cx, cy, cw, ch) = tile.core;
    for oy in cy * scale..(cy + ch) * scale {
        for ox in cx * scale..(cx + cw) * scale {
            let classical = classical_at(src, grid, ox, oy);
            let pixel = match plane {
                None => classical,
                Some(plane) => {
                    // Non-negative: the read origin is the core's less the pad.
                    let mx = (i64::from(ox) - tile.read.0 * i64::from(scale)) as usize * k;
                    let my = (i64::from(oy) - tile.read.1 * i64::from(scale)) as usize * k;
                    let mut pixel = classical;
                    for (c, value) in pixel.iter_mut().enumerate().take(3) {
                        let mut sum = 0.0_f32;
                        for j in 0..k {
                            let row = c * area + (my + j) * side + mx;
                            for cell in &plane[row..row + k] {
                                sum += cell;
                            }
                        }
                        *value = T::quantize((sum / cells).clamp(0.0, 1.0) * T::MAX);
                    }
                    pixel
                }
            };
            out[oy as usize * width + ox as usize] = pixel;
        }
    }
}
