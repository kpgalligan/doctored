//! A PSD vector mask's COVERAGE under its per-subpath operations (KERNEL):
//! what a `vmsk` Doctored cannot hold as a live vector mask draws, for an
//! import that keeps the picture as a raster mask (`psd_paths`).
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

/// The accumulator a path starts from over a window of `len` samples (u16
/// coverage, 65535 a wholly covered pixel): no pixels, or with
/// `start_full` all of them (module doc).
pub(crate) fn start(len: usize, start_full: bool) -> Vec<u16> {
    vec![if start_full { u16::MAX } else { 0 }; len]
}

/// Combines one subpath's own `coverage` (the accumulator's window) into
/// `acc` by `op` (module doc). The accumulator holds whole u16 samples
/// between subpaths, so one plane of scratch serves a path of any length;
/// the rounding that costs is half a u16 step per subpath.
pub(crate) fn apply(acc: &mut [u16], op: SubpathOp, coverage: &[u16]) {
    for (c, &s) in acc.iter_mut().zip(coverage) {
        let (c0, s) = (f64::from(*c) / 65535.0, f64::from(s) / 65535.0);
        let combined = match op {
            SubpathOp::Combine => c0 + s - c0 * s,
            SubpathOp::Subtract => c0 * (1.0 - s),
            SubpathOp::Intersect => c0 * s,
            SubpathOp::Exclude => c0 + s - 2.0 * c0 * s,
        };
        *c = (combined.clamp(0.0, 1.0) * 65535.0).round() as u16;
    }
}

/// Inverts `acc` when the vector mask's flags say inverted (module doc).
pub(crate) fn finish(acc: &mut [u16], invert: bool) {
    if invert {
        for c in acc.iter_mut() {
            *c = u16::MAX - *c;
        }
    }
}
