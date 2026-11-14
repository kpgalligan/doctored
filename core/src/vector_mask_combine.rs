//! A PSD vector mask's COVERAGE under its per-subpath operations (KERNEL):
//! what a `vmsk` Doctored cannot hold as a live vector mask draws, for an
//! import that keeps the picture as a raster mask (`psd_vector_mask_bake`).
//!
//! A path's subpaths combine in file order, each by the operation its length
//! record names (`psd_path_records`): 1 adds (combine), 2 subtracts, 3
//! intersects and 0 excludes (xor), starting from no pixels, or from all of
//! them when the initial fill record says so or the FIRST subpath subtracts
//! (Photoshop's 8-bit rendering of such a mask is everything outside the
//! subpath, the family's `Ve`). On coverage `c` so far and a
//! subpath's own coverage `s`, both fractions of a pixel: add `c + s - c s`,
//! subtract `c (1 - s)`, intersect `c s`, exclude `c + s - 2 c s` — the
//! union, difference, intersection and symmetric difference of independent
//! area coverages, which are exact for the whole pixels inside and outside
//! every edge and blend only the anti-aliased edge pixels. A vector mask
//! whose flags say inverted is inverted last. Evidence: the
//! `authored_exchange` family's 16-bit raster layers, whose files carry no
//! rendering of these masks, and its 8-bit twins, which do (the tests
//! compare the two).
//!
//! **Windows.** A subpath is drawn only over a [`Window`] of the canvas, not
//! the whole of it, and the picture is the one a whole-canvas draw makes.
//! Outside a subpath's own window (its control hull, padded and clamped to
//! the canvas by the caller) `s` is 0, and there add, subtract and exclude
//! give back `c` exactly (`c / 65535 · 65535` rounds to `c`), while
//! intersect gives 0. The accumulator also keeps a LIVE box holding every
//! nonzero sample (the whole canvas from a full start, nothing from an
//! empty one), where subtract and intersect change nothing outside it
//! (`0 · x` is 0). So [`charge`], the one rule both the drawing cap and the
//! draw follow, draws an add or an exclude over the hull and widens the live
//! box to hold it; a subtract over the hull inside the live box; and an
//! intersect over the hull inside the live box, clearing the rest of the
//! live box, which then shrinks to that window. Its samples are the
//! window's, but an intersect's are the whole live box it combines and
//! clears; none is ever more than the canvas. [`Accumulator::start`] and
//! [`Accumulator::finish`] are one canvas pass per mask, outside the charge.

/// One subpath's operation word (`psd_path_records`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubpathOp {
    Exclude,
    Combine,
    Subtract,
    Intersect,
}

impl SubpathOp {
    /// The length record's operation word, `None` for an unknown one.
    pub(crate) fn from_word(word: i16) -> Option<Self> {
        match word {
            0 => Some(Self::Exclude),
            1 => Some(Self::Combine),
            2 => Some(Self::Subtract),
            3 => Some(Self::Intersect),
            _ => None,
        }
    }
}

/// A rectangle of canvas samples: `width` × `height` whose top-left is
/// canvas pixel (`x`, `y`). Empty when either side is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Window {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Window {
    /// The whole of a `width` × `height` canvas.
    pub(crate) fn canvas(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Its samples.
    pub(crate) fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The samples both hold; empty when they do not meet.
    fn intersect(self, other: Self) -> Self {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        if self.is_empty() || other.is_empty() || x1 <= x0 || y1 <= y0 {
            return Self::default();
        }
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }

    /// The smallest window holding both; an empty one adds nothing.
    fn union_box(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.width).max(other.x + other.width);
        let y1 = (self.y + self.height).max(other.y + other.height);
        Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    }
}

/// THE window rule (module doc): for a subpath combined by `op` whose own
/// window is `hull`, the window to draw it over and the samples charged for
/// it, moving `live` (the box of every nonzero sample so far) past it. The
/// drawing cap (`psd_vector_mask_bake::bakes`) folds it over a mask's
/// subpaths and [`Accumulator::apply`] draws by it, so the two agree.
pub(crate) fn charge(live: &mut Window, op: SubpathOp, hull: Window) -> (Window, u64) {
    match op {
        SubpathOp::Combine | SubpathOp::Exclude => {
            *live = live.union_box(hull);
            (hull, hull.area())
        }
        SubpathOp::Subtract => {
            let window = hull.intersect(*live);
            (window, window.area())
        }
        SubpathOp::Intersect => {
            let window = hull.intersect(*live);
            let samples = live.area();
            *live = window;
            (window, samples)
        }
    }
}

/// The coverage a path builds over a `width` × `height` canvas (u16, 65535 a
/// wholly covered pixel), row-major, and its live box (module doc).
pub(crate) struct Accumulator {
    samples: Vec<u16>,
    width: u32,
    live: Window,
}

impl Accumulator {
    /// No pixels, or with `full` all of them (module doc).
    pub(crate) fn start(width: u32, height: u32, full: bool) -> Self {
        let canvas = Window::canvas(width, height);
        Self {
            samples: vec![if full { u16::MAX } else { 0 }; canvas.area() as usize],
            width,
            live: if full { canvas } else { Window::default() },
        }
    }

    /// Combines one subpath into the accumulator by `op` (module doc). `hull`
    /// is the subpath's own window; `draw` is asked for the subpath's
    /// coverage over the window [`charge`] names, row-major, only when that
    /// window is not empty, and its refusal is this call's. The accumulator
    /// holds whole u16 samples between subpaths, so one plane of scratch
    /// serves a path of any length; the rounding that costs is half a u16
    /// step per subpath.
    pub(crate) fn apply(
        &mut self,
        op: SubpathOp,
        hull: Window,
        draw: impl FnOnce(Window) -> Result<Vec<u16>, String>,
    ) -> Result<(), String> {
        let before = self.live;
        let (window, _) = charge(&mut self.live, op, hull);
        if op == SubpathOp::Intersect {
            self.clear_outside(before, window);
        }
        if window.is_empty() {
            return Ok(());
        }
        let coverage = draw(window)?;
        if coverage.len() as u64 != window.area() {
            return Err("internal error: a subpath's coverage does not fill its window".into());
        }
        let (w, run) = (self.width as usize, window.width as usize);
        for (row, own) in coverage.chunks_exact(run).enumerate() {
            let at = (window.y as usize + row) * w + window.x as usize;
            for (c, &s) in self.samples[at..at + run].iter_mut().zip(own) {
                *c = combined(op, *c, s);
            }
        }
        Ok(())
    }

    /// Zeroes every sample of `live` outside `kept`: what an intersect does
    /// where its subpath covers nothing.
    fn clear_outside(&mut self, live: Window, kept: Window) {
        let w = self.width as usize;
        for y in live.y..live.y + live.height {
            let row = y as usize * w;
            let inside = !kept.is_empty() && (kept.y..kept.y + kept.height).contains(&y);
            let (from, to) = (live.x as usize, (live.x + live.width) as usize);
            if inside {
                let (keep0, keep1) = (kept.x as usize, (kept.x + kept.width) as usize);
                self.samples[row + from..row + keep0].fill(0);
                self.samples[row + keep1..row + to].fill(0);
            } else {
                self.samples[row + from..row + to].fill(0);
            }
        }
    }

    /// The coverage, inverted when the vector mask's flags say so (module
    /// doc).
    pub(crate) fn finish(mut self, invert: bool) -> Vec<u16> {
        if invert {
            for c in self.samples.iter_mut() {
                *c = u16::MAX - *c;
            }
        }
        self.samples
    }
}

/// One sample `c` so far combined with a subpath's own `s` by `op` (module
/// doc), rounded to a whole u16.
fn combined(op: SubpathOp, c: u16, s: u16) -> u16 {
    let (c0, s) = (f64::from(c) / 65535.0, f64::from(s) / 65535.0);
    let combined = match op {
        SubpathOp::Combine => c0 + s - c0 * s,
        SubpathOp::Subtract => c0 * (1.0 - s),
        SubpathOp::Intersect => c0 * s,
        SubpathOp::Exclude => c0 + s - 2.0 * c0 * s,
    };
    (combined.clamp(0.0, 1.0) * 65535.0).round() as u16
}
