//! KERNEL: the WINDOW of an artwork component that owns a sample plane (a
//! `coverage` or `intrinsic_rgba8` component, RGB and CMYK alike).
//!
//! A component may declare `"window": [x, y, w, h]` in raster pixels; absent,
//! its window is the whole raster. Its samples cover only the window,
//! row-major (`w·h` u16 coverage samples, or `w·h·4` intrinsic RGBA8 bytes),
//! and the planes follow one another in component order, so a component's
//! offset is the sum of the earlier ones' sizes. Outside its window a coverage
//! component's coverage is 0 and an intrinsic component's pixel is transparent
//! (`[0, 0, 0, 0]`), which is exactly what a whole-raster plane holding the
//! window's samples embedded in zeros composites: a windowed list renders as
//! the zero-embedded whole-raster list, sample for sample.
//!
//! The bound is per component: at most `artwork_render::MAX_COMPONENT_SAMPLES`
//! samples each (64M; an intrinsic component's bytes count). There is no
//! whole-layer total: a list holds at most 256 components (`MAX_COMPONENTS`),
//! `dr_artwork_render`'s memory admission counts every sample the caller
//! hands it, and the host leases the planes it hands either render
//! (`NativeArtworkEvaluation.ownScratchBytes`).
//! Before, every plane covered the whole raster and the planes shared one
//! 64M total, so a text box of a few dozen colour runs refused although each
//! run inks a few glyphs.
//!
//! Refusals: a width or height below 1, a window not inside the raster, a
//! component over the bound, and buffers whose lengths differ from what the
//! windows name.
use crate::artwork_render::MAX_COMPONENT_SAMPLES;

/// A validated window: its origin and size in raster pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Window {
    /// The window `declared` names over a `raster` (width, height), or the
    /// whole raster when it names none; each pixel holds `per_pixel` samples
    /// (1 for coverage, 4 for intrinsic RGBA8). Refuses a window that is
    /// empty, not inside the raster, or over the per-component bound.
    pub(crate) fn place(
        declared: Option<[i64; 4]>,
        raster: (u32, u32),
        per_pixel: usize,
    ) -> Result<Self, String> {
        let (x, y, w, h) = match declared {
            None => (0, 0, i64::from(raster.0), i64::from(raster.1)),
            Some([x, y, w, h]) => (x, y, w, h),
        };
        let inside = |origin: i64, size: i64, extent: u32| {
            origin >= 0 && size >= 1 && origin.saturating_add(size) <= i64::from(extent)
        };
        if !inside(x, w, raster.0) || !inside(y, h, raster.1) {
            return Err(
                "artwork component window must have a positive size inside the raster".into(),
            );
        }
        let window = Self {
            x: x as usize,
            y: y as usize,
            width: w as usize,
            height: h as usize,
        };
        if window
            .pixels()
            .checked_mul(per_pixel)
            .is_none_or(|samples| samples > MAX_COMPONENT_SAMPLES)
        {
            return Err("artwork component exceeds 64,000,000 samples".into());
        }
        Ok(window)
    }

    /// A plane-owning component's window and whether its plane is RGBA8
    /// (`rgba`, 4 bytes a pixel) rather than u16 coverage: [`Self::place`]
    /// for the RGB and CMYK lists alike.
    pub(crate) fn plane(
        declared: Option<[i64; 4]>,
        raster: (u32, u32),
        rgba: bool,
    ) -> Result<Option<(Self, bool)>, String> {
        Self::place(declared, raster, if rgba { 4 } else { 1 }).map(|w| Some((w, rgba)))
    }

    /// The window's pixel count.
    pub(crate) fn pixels(&self) -> usize {
        self.width * self.height
    }

    /// Raster pixel `(x, y)`'s index inside the window, row-major, or `None`
    /// outside it. The whole-raster window answers `y·width + x`.
    pub(crate) fn at(&self, x: usize, y: usize) -> Option<usize> {
        let (dx, dy) = (x.checked_sub(self.x)?, y.checked_sub(self.y)?);
        (dx < self.width && dy < self.height).then(|| dy * self.width + dx)
    }
}

/// The sample and intrinsic-byte totals of a component list's windows, in
/// order: `windows` yields each plane-owning component's window and whether it
/// is intrinsic. What the caller's buffers must hold exactly.
pub(crate) fn totals(windows: impl IntoIterator<Item = (Window, bool)>) -> (usize, usize) {
    windows
        .into_iter()
        .fold((0, 0), |(samples, intrinsic), (window, rgba)| {
            if rgba {
                (samples, intrinsic + window.pixels() * 4)
            } else {
                (samples + window.pixels(), intrinsic)
            }
        })
}
