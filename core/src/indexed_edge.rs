//! KERNEL. The Indexed Color painting edge rule (#1 Indexed phase 2b,
//! `designs/indexed-mode.md`): how a paint op's RGB result goes back
//! onto the palette.
//!
//! A paint op on an Indexed document runs on its RGB storage, then this
//! kernel re-indexes the result against the plane from before the op:
//!
//! - an UNCHANGED pixel (its straight RGBA8 equal to its old index's render:
//!   the palette colour at alpha 255, or alpha 0 at the transparent index t)
//!   keeps its index;
//! - a changed pixel with alpha below 128 takes [`clear_index`] (t, else the
//!   entry nearest the background colour since phase 3a, below);
//! - a changed pixel at alpha 128 or above whose old index is t takes E, the
//!   palette entry nearest the ink (`indexed_palette::NearestMap`: least
//!   squared RGB distance, ties to the lowest index, never t). This branch
//!   is decided BEFORE the rule, because t's palette colour is the matte, not
//!   a backdrop to project against;
//! - every other changed pixel takes THRESH: E when the pixel's coverage
//!   α ≥ [`THRESH_K`]/255, else its old index. α is the projection of the
//!   result c onto the ink from the backdrop s (the old index's colour):
//!   α = clamp(Σ(c−s)(ink−s) / Σ(ink−s)², 0, 1), with α = 1 when the ink IS
//!   the backdrop (Σ(ink−s)² = 0: no division, no NaN).
//!
//! Why THRESH at k = 131. Photoshop's take1 (`scripts/photoshop-probe/
//! indexed_mode_take.py`) showed every full-ink pixel takes E and every
//! changed edge pixel takes either E or its own backdrop, never a third
//! entry. No declared candidate (nearest, mixes, Floyd–Steinberg, ordered
//! and error-diffusion dithers, distance thresholds) reached the 99 % gate,
//! and take2 painted the same stroke twice from one history state with two
//! different edges (64 of 803 pixels), so Photoshop's edge is not
//! deterministic and no exact rule exists. The controller's ruling
//! (RULING-1, step 3) ships THRESH at take1's best k, 131 (751 of 806
//! changed pixels, 93.2 %; take2's twin 93.3 %, its fresh stroke 85.8 %;
//! every full-ink pixel exact on all three) as a deterministic rule with
//! that residual disclosed.
//!
//! The comparison is exact in integers: α ≥ k/255 ⇔ 255·num ≥ k·den for
//! den > 0, and the clamp needs no code (num ≥ den gives 255·num ≥ k·den
//! for every k ≤ 255; num < 0 fails it for every k > 0).
//!
//! # Phase 2c: the ink and the backdrop generalised
//!
//! Since #1 Indexed phase 2c every other admitted edit (Fill, Stroke, the
//! Paint Bucket, the Eraser, Cut, Clear and the resampling geometry) maps
//! back through the same rule, with the ink named by [`EdgeInk`] and the
//! backdrop optional. Per changed pixel (every pixel when there is no
//! `before`; an unchanged one keeps its index):
//!
//! - alpha below 128 takes [`clear_index`]: t, and with no t (since phase
//!   3a, replacing phase 2c's refusal) E of [`BACKGROUND`], the entry
//!   nearest the default background colour, as a Background layer paints
//!   an erase, a canvas extension or an uncovered
//!   transform area: an Indexed document without a transparent colour
//!   cannot hold a clear pixel, so the background colour stands in for one;
//! - [`EdgeInk::Clear`] (the Eraser, Cut, Clear) keeps the old index: an
//!   erase moves alpha only, and a pixel still at alpha 128 or above is the
//!   colour it was. It needs a `before`;
//! - an old index t, or no `before` (a resample, whose pixels have no
//!   backdrop to project against), takes E: the entry nearest the ink
//!   ([`EdgeInk::Fixed`]) or the pixel's own colour ([`EdgeInk::Own`]),
//!   never t;
//! - otherwise `Fixed` takes THRESH as above and `Own` the entry nearest its
//!   own colour.
//!
//! A `Fixed` ink with a `before` is the 2b Brush's rule exactly.
//!
//! # Phase 3b: the other blend modes and the background colour
//!
//! Since #1 Indexed phase 3b the Brush, Fill, Stroke and the Paint Bucket
//! paint in every blend mode they take on RGB. A mode other than Normal is
//! [`EdgeInk::Blend`]: per changed pixel over an opaque backdrop s, the
//! target T is the mode's full-strength colour over s at 8-bit codes
//! (`blend::blend_kind`: a separable mode per channel, a non-separable one on
//! the triple, each round(255·B(s/255, ink/255)); Dissolve's full strength is
//! the ink itself), and THRESH runs against T instead of the ink: α ≥ k/255
//! takes the entry nearest T, else the old index (T = s gives α = 1). Why T:
//! a blend mode's ink is not one colour but a colour per backdrop, and T is
//! exactly what a full-coverage pixel of that backdrop becomes, so a
//! full-ink pixel takes the entry nearest its own painted colour, as a
//! Normal one does. A t backdrop composites the ink itself (an alpha-0
//! backdrop shows the source as it is), so it takes E of the ink, as does a
//! pixel with no backdrop.
//!
//! The background colour is no longer fixed: [`clear_index`] takes it (the
//! host's background swatch, or white when the request names none,
//! [`BACKGROUND`]), and [`reindex`] looks it up only on the first pixel
//! that ends clear, through `indexed_palette::nearest_entry`, so a no-t
//! re-index with no clear pixel builds no second memo.

use crate::blend::{blend_kind, BlendKind, BlendMode};
use crate::indexed_palette::{nearest_entry, NearestMap, Rgb};

/// The default background colour (D), white: the colour a clear
/// pixel becomes on a document with no transparent index when the request
/// names none (module doc).
pub(crate) const BACKGROUND: Rgb = [255, 255, 255];

/// The index a pixel left clear (alpha below 128) takes (module doc): the
/// transparent index t, else E of `background`, the nearest entry, ties
/// to the lowest index. The one home of the background rule.
pub(crate) fn clear_index(palette: &[Rgb], transparent: Option<u8>, background: Rgb) -> u8 {
    match transparent {
        Some(t) => t,
        None => nearest_entry(palette, None, background),
    }
}

/// The ink a changed pixel is projected against (module doc).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeInk {
    /// One paint colour, document 8-bit codes: THRESH against the backdrop.
    Fixed([u8; 3]),
    /// One paint colour in a blend mode other than Normal (phase 3b, module
    /// doc): THRESH against the mode's full-strength colour over the
    /// backdrop.
    Blend([u8; 3], BlendMode),
    /// Each pixel's own colour: the nearest entry (a resample).
    Own,
    /// An erase: alpha only, the old index kept.
    Clear,
}

/// The full-strength colour of `ink` in `mode` over backdrop `s`, 8-bit
/// codes (module doc): `blend::blend_kind`'s function at unit scale, each
/// channel rounded back to a code; Dissolve's is the ink.
fn blend_target(s: Rgb, ink: Rgb, mode: BlendMode) -> Rgb {
    let unit = |v: [u8; 3]| v.map(|c| f32::from(c) / 255.0);
    let code = |v: f32| (f64::from(v).clamp(0.0, 1.0) * 255.0).round() as u8;
    match blend_kind(mode) {
        BlendKind::Separable(b) => {
            [0, 1, 2].map(|c| code(b(f32::from(s[c]) / 255.0, f32::from(ink[c]) / 255.0)))
        }
        BlendKind::NonSeparable(b) => b(unit(s), unit(ink)).map(code),
        BlendKind::Dissolve => ink,
    }
}

/// THRESH's k (module doc): a changed edge pixel takes E when its coverage
/// α ≥ 131/255, take1's fitted best.
pub(crate) const THRESH_K: i64 = 131;

/// Below this alpha a changed pixel is transparent (the transparent index's
/// side), as Mode ▸ Indexed Color's Transparency reads alpha
/// (`indexed_palette::prepare`).
const ALPHA_OPAQUE_FROM: u8 = 128;

/// What one re-index reads (module doc).
pub(crate) struct EdgeInput<'a> {
    pub(crate) palette: &'a [Rgb],
    pub(crate) transparent: Option<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The plane before the op, `width·height` indices; `None` when the
    /// result has no backdrop (a resample: every pixel is changed).
    pub(crate) before: Option<&'a [u8]>,
    /// The op's ink.
    pub(crate) ink: EdgeInk,
    /// The colour a clear pixel's entry is nearest with no transparent
    /// index ([`clear_index`]).
    pub(crate) background: Rgb,
}

/// Whether a changed pixel `c` over backdrop `s` takes E (THRESH, module doc).
fn takes_ink(c: Rgb, s: Rgb, ink: Rgb) -> bool {
    let (mut num, mut den) = (0i64, 0i64);
    for k in 0..3 {
        let d = i64::from(ink[k]) - i64::from(s[k]);
        num += (i64::from(c[k]) - i64::from(s[k])) * d;
        den += d * d;
    }
    den == 0 || 255 * num >= THRESH_K * den
}

/// The plane after an op (module doc). `row(y, buf)` fills row `y` of the
/// op's result as straight RGBA8 (`indexed_palette::code8` of its samples).
pub(crate) fn reindex(
    input: &EdgeInput<'_>,
    mut row: impl FnMut(u32, &mut [[u8; 4]]) -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    let width = input.width as usize;
    let count = width
        .checked_mul(input.height as usize)
        .ok_or("the Indexed Color plane is too large")?;
    let entries = input.palette.len();
    if let Some(before) = input.before {
        if before.len() != count {
            return Err("the Indexed Color index plane does not match the canvas".into());
        }
        if before.iter().any(|&i| usize::from(i) >= entries) {
            return Err("an Indexed Color index is outside the palette".into());
        }
    } else if input.ink == EdgeInk::Clear {
        return Err("an Indexed Color erase needs the plane from before it".into());
    }
    if input.transparent.is_some_and(|t| usize::from(t) >= entries) {
        return Err("an Indexed Color index is outside the palette".into());
    }
    // Looked up on the first pixel that ends clear (module doc).
    let mut clear = None;
    let mut map = NearestMap::new(input.palette.to_vec(), input.transparent.map(usize::from))?;
    // E of a `Fixed` or `Blend` ink, looked up once.
    let fixed = match input.ink {
        EdgeInk::Fixed(ink) | EdgeInk::Blend(ink, _) => Some(map.index(ink)),
        EdgeInk::Own | EdgeInk::Clear => None,
    };
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| "the Indexed Color plane could not be allocated")?;
    let mut line = vec![[0u8; 4]; width];
    for y in 0..input.height {
        row(y, &mut line)?;
        let start = y as usize * width;
        for (x, px) in line.iter().enumerate() {
            let old = input.before.map(|before| before[start + x]);
            let backdrop = old.map(|old| {
                let s = input.palette[usize::from(old)];
                let clear = Some(old) == input.transparent;
                (old, s, clear)
            });
            if let Some((old, s, clear)) = backdrop {
                if *px == [s[0], s[1], s[2], if clear { 0 } else { 255 }] {
                    out.push(old);
                    continue;
                }
            }
            let own = [px[0], px[1], px[2]];
            let index = if px[3] < ALPHA_OPAQUE_FROM {
                *clear.get_or_insert_with(|| {
                    clear_index(input.palette, input.transparent, input.background)
                })
            } else {
                match (backdrop, input.ink) {
                    // An erase moves alpha only (`Clear` always has a
                    // backdrop: checked above).
                    (Some((old, ..)), EdgeInk::Clear) => old,
                    (Some((old, s, false)), EdgeInk::Fixed(ink)) => {
                        if takes_ink(own, s, ink) {
                            fixed.unwrap_or(old)
                        } else {
                            old
                        }
                    }
                    (Some((old, s, false)), EdgeInk::Blend(ink, mode)) => {
                        let target = blend_target(s, ink, mode);
                        if takes_ink(own, s, target) {
                            map.index(target)
                        } else {
                            old
                        }
                    }
                    (Some((_, _, false)), _) => map.index(own),
                    // A t backdrop, or none: E, before the rule.
                    _ => fixed.unwrap_or_else(|| map.index(own)),
                }
            };
            out.push(index);
        }
    }
    Ok(out)
}

/// [`reindex`] over a whole result held in memory with a `Fixed` ink and a
/// backdrop (the 2b Brush): the black-box tests' seam
/// (`core/tests/indexed_paint.rs`, `indexed_photoshop_take.rs`).
#[doc(hidden)]
pub fn reindex_codes(
    palette: &[[u8; 3]],
    transparent: Option<u8>,
    width: u32,
    height: u32,
    before: &[u8],
    after: &[[u8; 4]],
    ink: [u8; 3],
) -> Result<Vec<u8>, String> {
    reindex_codes_with(
        palette,
        transparent,
        width,
        height,
        Some(before),
        after,
        EdgeInk::Fixed(ink),
    )
}

/// [`reindex`] over a whole result held in memory, any ink, the backdrop
/// optional: the phase 2c tests' seam (`core/tests/indexed_edits.rs`).
#[doc(hidden)]
pub fn reindex_codes_with(
    palette: &[[u8; 3]],
    transparent: Option<u8>,
    width: u32,
    height: u32,
    before: Option<&[u8]>,
    after: &[[u8; 4]],
    ink: EdgeInk,
) -> Result<Vec<u8>, String> {
    reindex_codes_on(
        palette,
        transparent,
        width,
        height,
        before,
        after,
        ink,
        BACKGROUND,
    )
}

/// [`reindex_codes_with`] with the background colour named (phase 3b): the
/// swatch tests' seam (`core/tests/indexed_background.rs`).
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn reindex_codes_on(
    palette: &[[u8; 3]],
    transparent: Option<u8>,
    width: u32,
    height: u32,
    before: Option<&[u8]>,
    after: &[[u8; 4]],
    ink: EdgeInk,
    background: [u8; 3],
) -> Result<Vec<u8>, String> {
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or("the Indexed Color plane is too large")?;
    if after.len() != before.map_or(count, <[u8]>::len) {
        return Err("the result does not match the canvas".into());
    }
    let input = EdgeInput {
        palette,
        transparent,
        width,
        height,
        before,
        ink,
        background,
    };
    let width = width as usize;
    reindex(&input, |y, buf| {
        let start = y as usize * width;
        buf.copy_from_slice(&after[start..start + width]);
        Ok(())
    })
}
