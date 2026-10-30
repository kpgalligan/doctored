//! Indexed Color's ordered dithers, Pattern and Noise (#1 Indexed phase
//! 3c, KERNEL; `designs/indexed-mode.md`). Adobe has not published
//! either, so both are DECLARED CANDIDATES. Phase 3d's Photoshop take
//! (`scripts/photoshop-probe/indexed_palette_take.py`, `palette-take1`)
//! kept both, its numbers disclosed: Pattern MISSED the gate (≥ 99 % exact on
//! every converted case) — this rule reproduces 99.21 % of Photoshop's Web
//! ramp but 62.88 % of its System (Mac OS) one, and no fitted Bayer or
//! per-cell table passed both — and Photoshop refused every Noise
//! conversion through the action descriptor, so Noise was not measured.
//!
//! Each opaque pixel gets one threshold `t` in [0, 1) from its canvas
//! position and is moved by the same offset on r, g and b:
//! `w_c = clamp(src_c + (t − ½)·DITHER_SPREAD, 0, 255)`, then indexed to the
//! entry nearest `round(w)`. A transparent pixel takes the transparent index.
//!
//! * **Pattern** reads the 8 × 8 Bayer matrix from the canvas origin,
//!   `t = (2·M[y mod 8][x mod 8] + 1)/128`. The matrix is Bitmap's Pattern
//!   Dither's (`bitmap_method::bayer` at `bitmap_method::PATTERN_ORDER`, the
//!   master copy); this module is its Indexed sibling, not a second matrix.
//! * **Noise** reads Dissolve's canvas-absolute hash
//!   (`blend::dissolve_threshold`, the master copy), so it is deterministic
//!   and needs no seed or scratch.
//!
//! The nearest entry is `indexed_palette::NearestMap`'s (the master copy).
//! With the Exact palette `indexed_convert` runs Dither None instead: every
//! pixel already has its own entry, as Diffusion's zero error leaves it.

use crate::bitmap_method::{bayer, PATTERN_ORDER};
use crate::indexed_palette::{NearestMap, Rgb};

/// The threshold's reach in 8-bit codes: the Web cube's step, 255/5 = 51.
/// Declared, and kept by phase 3d's take (module doc): no fitted spread
/// passed on both of Photoshop's Pattern ramps, and Noise was refused.
pub(crate) const DITHER_SPREAD: f64 = 51.0;

/// The two ordered dithers (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ordered {
    Pattern,
    Noise,
}

/// The threshold in [0, 1) at canvas pixel (x, y) (module doc).
pub(crate) fn threshold(kind: Ordered, x: u32, y: u32) -> f64 {
    match kind {
        Ordered::Pattern => {
            let cells = PATTERN_ORDER * PATTERN_ORDER;
            f64::from(2 * bayer(PATTERN_ORDER, x, y) + 1) / f64::from(2 * cells)
        }
        Ordered::Noise => f64::from(crate::blend::dissolve_threshold(i64::from(x), i64::from(y))),
    }
}

/// One row `y` of prepared pixels (`None` transparent) dithered onto the
/// map's palette (module doc).
pub(crate) fn ordered_row(
    map: &mut NearestMap,
    kind: Ordered,
    y: u32,
    source: &[Option<Rgb>],
    transparent: u8,
    out: &mut [u8],
) {
    for (x, (pixel, slot)) in source.iter().zip(out.iter_mut()).enumerate() {
        *slot = match pixel {
            Some(src) => {
                let offset = (threshold(kind, x as u32, y) - 0.5) * DITHER_SPREAD;
                let w = src.map(|c| (f64::from(c) + offset).clamp(0.0, 255.0).round() as u8);
                map.index(w)
            }
            None => transparent,
        };
    }
}
