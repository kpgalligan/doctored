//! Satin (Layer Style > Satin): the sheen of folded fabric. Two
//! copies of the shape's soft matte (the box-pair law, [`satin_law_applies`])
//! are pushed apart along `angle` by `distance` — one copy each way — mapped
//! through the contour each, and combined by their difference `|C(a) −
//! C(b)|`, which is what one covers and the other does not; `invert` takes
//! the complement. The result is clipped to the shape and composited above
//! the pixels with the effect's own blend mode (Multiply by default) and
//! opacity (0.5), as one Interior contribution.
//!
//! # The algorithm
//!
//! `p` = THE LAW's matte at spread 0 wherever `size > 0`
//! (`style_matte::law_matte_with_job(coverage, size, 0, AntiAliased)`, its
//! band at `n = 0` the coverage itself, then the box pair at `w =
//! law_split(size, 0).1`, rounded once by `law_byte`); at size 0 today's
//! `MATTE.blur_with_job` at sigma 0, the coverage itself;
//! `(dx, dy) = light_offset(angle, distance)`;
//! `a = shift_plane(p, dx, dy, 0)`, `b = shift_plane(p, -dx, -dy, 0)`;
//! the contour maps `a` and `b` each ([`CONTOUR_PER_COPY`]; Linear skips
//! it); per pixel `s = |a - b|` (`SATIN_COMBINE_ABS`; the soft XOR `a + b -
//! 2ab` with `a`, `b` in 0..1 is the member it replaced); `invert` → `1 -
//! s`; then `s * coverage`. Every plane primitive is shared — the ONE law,
//! the ONE shift, the ONE contour map — so this module holds no pixel math
//! of its own beyond the combine.
//!
//! # Why these numbers
//!
//! - **The law and the per-copy contour (#578 part 3).** Declared in
//!   `scripts/photoshop-probe/effect_kernels_578_p3_take.py` before its
//!   takes: SA1 (the law's matte, argued from the shadows' D1 and I1, which
//!   replaced the same Gaussian G1/2.5 with it and were confirmed) and SA2
//!   (the contour on each shifted copy before the combine, contour-557's
//!   finding), judged jointly as SA2l. On the 8-bit identify take
//!   (`effect-kernels-578-p3/identify1`, 80 satin rows on a binary rect, a
//!   binary disc and an anti-aliased ellipse) and every existing satin row,
//!   SA2l SHIPPED by rule (ii): mean of row means 8.00 → 0.10 codes held-out
//!   (66 of 80 rows within 2 / 0.5, none before) and 12.03 → 0.14 existing,
//!   none rising (contour-557's Cone 250 / 30.1 → 2.1 / 0.43). The fresh
//!   take at 8 and 16 bits (`effect-kernels-578-p3/take1`, an ellipse and a
//!   rounded rect) CONFIRMED it by rule (ii): 80 rows 2.38 → 0.054, none
//!   rising, five contoured 8-bit rows over the gate's max by at most 0.03
//!   codes. Those are the runner's model's numbers: this kernel rounds the
//!   law's matte to a byte (`law_byte`) before the contour, so its 24
//!   contoured rows (10 identify, 14 confirm) read 2.2-5 codes max, means
//!   up to 0.29, each pinned at its measured number in
//!   `tests/style_effect_kernels_578_p3_take.rs`. SA1 alone CONFIRMED by rule (i) on its 64 Linear rows (0.544 →
//!   0.048). The runner's RECORDED section holds every row.
//! - **`|a - b|` (#578).** Photoshop combines the two offset copies with
//!   an exclusive-or-like difference: where both copies cover, or neither
//!   does, there is no sheen. Both `|a - b|` and the soft XOR `a + b - 2ab`
//!   equal XOR at every corner of the unit square and are symmetric in `a`
//!   and `b`; the soft XOR adds `2 min(a, b) (1 - max(a, b))` where the two
//!   blurred copies overlap, a sheen Photoshop does not draw. `G1/2.5 ·
//!   Rnone · γ 1 · Bbefore · Xabs` (sigma `size / 2.5`, the blur before the
//!   combine) was fitted offline by `soft_effects_578_fit.py` on
//!   effects-578 take1 against declared families (Gaussian ratios 1/2,
//!   1/2.5, 1/3, two box blurs, three range maps, five gammas, the blur
//!   before or after a hard XOR, the soft or absolute combine): by the
//!   model A3's default satin (size 14, distance 11, 19°, invert) went from
//!   30.83 / 0.995 to 3.17 / 0.122 codes at 8 bits (31.01 / 1.009 to 3.10 /
//!   0.132 at 16), the hard row unchanged, and soft-effects-578 take1's
//!   held-out rows confirm it (size 6 at 60° 30.41 / 0.449 -> 2.93 / 0.084;
//!   size 20 at 135° without invert 31.05 / 1.470 -> 3.23 / 0.163; size 10
//!   at 90° 30.83 / 1.301 -> 2.80 / 0.118). The module's SHIP rows are
//!   recorded in the runner's RECORDED section. The Gaussian matte that
//!   fit used is the law's since part 3 (above); [`MATTE`] keeps size 0.
//! - **Integer form.** With `a`, `b` in 0..=255, `|a - b|` is exact on the
//!   bytes. The soft XOR is `((a + b) * 255 - 2ab + 127) / 255`, one
//!   rounding: the numerator is never negative
//!   (`a (255 - b) + b (255 - a) >= 0`) and never exceeds `255 * 255`
//!   (`255² - 255 (a + b) + 2ab = (255 - a)(255 - b) + ab >= 0`), so the
//!   quotient is a byte.
//! - **Symmetric shift.** The copies sit at `+(dx, dy)` and `-(dx, dy)`, so
//!   the pattern stays centred on the shape and an angle flipped by 180°
//!   renders identically — which copy gets which sign is immaterial since
//!   the combine is symmetric. The shifted-in region is
//!   filled with 0 (outside the layer is transparent); `LayerStyle::pad`
//!   reserves [`satin_reach`] (the matte's reach plus `ceil(distance)`)
//!   around the shape, so a shift by at most `distance` only ever reads
//!   real zeros and the plane renders exactly as it would on an infinite
//!   canvas.
//! - **Distance 0.** `a == b`, so `s = 0`: no sheen at all (the soft XOR
//!   left `2a (1 - a)`, a faint ridge along a blurred edge); with `invert`
//!   the whole shape is painted.
//! - **No global light.** The Satin pane has no "Use Global Light"
//!   checkbox; `angle` is always the effect's own, so the render context
//!   is not read.
//!
//! - **Contour** (`style_contour`; the site since #578 part 3): each
//!   shifted copy is mapped in place, every pixel, before the combine
//!   ([`CONTOUR_PER_COPY`]); the combined plane before `invert` is the site
//!   it replaced (contour-557's best fit, which missed). Linear skips the
//!   pass, so a Linear satin's only change is the law's matte.
//! - **Locality.** The law reads `law_reach(size, 0)` px (`w`: the band
//!   adds nothing at `n = 0`); the box pair reports its rows on the job's
//!   range at stage 6, as the Gaussian did, and the combine still polls
//!   once per row (13A-5b-i). The law's f64 planes fit inside the scratch
//!   plan every styled render charges ([`satin_sigma`] reports 0 under the
//!   law, the shadows' precedent in `style_matte`).

use crate::job_control::JobContext;
use crate::style::{EffectKind, Satin};
use crate::style_contour::{effect_lut, map_plane_with_job};
use crate::style_matte::{
    law_applies, law_byte, law_matte_with_job, law_reach, LawBand, Matte, MatteRange,
};
use crate::style_render::{
    blur_reach, invert_plane, light_offset, multiply_planes, shift_plane, slot_color, Contribution,
    Placement, RenderContext, Shape,
};

/// The satin's soft matte where the law does not run (size 0), `G1/2.5 ·
/// Rnone · γ 1` (module doc): the ONE sizing of its blur (`split(size,
/// 0)`), read through [`satin_sigma`] and [`satin_reach`].
pub(crate) const MATTE: Matte = Matte {
    ratio: 0.4,
    range: MatteRange::None,
    gamma: 1.0,
};

/// The two copies combine as `|a - b|` ([`abs_xor`], #578), not the soft
/// XOR ([`soft_xor`]) it replaced (module doc).
const SATIN_COMBINE_ABS: bool = true;

/// The contour maps each shifted copy before the combine (module doc, #578
/// part 3's SA2); false is the site it replaced, the combined plane before
/// `invert`.
const CONTOUR_PER_COPY: bool = true;

/// Whether the box-pair law renders the satin's matte (module doc):
/// `style_matte::law_applies(size, 0)`, every size above 0.
pub(crate) fn satin_law_applies(size: f32) -> bool {
    law_applies(size, 0.0)
}

/// How far the satin reads or writes from any pixel (`Effect::reach`):
/// `law_reach(size, 0) + ceil(distance)` under the law, else the Gaussian's
/// `blur_reach(sigma) + ceil(distance)`. Every addition saturates.
pub(crate) fn satin_reach(size: f32, distance: f32) -> u32 {
    let shift = distance.max(0.0).ceil() as u32;
    let matte = if satin_law_applies(size) {
        law_reach(size, 0.0)
    } else {
        blur_reach(satin_sigma(size))
    };
    matte.saturating_add(shift)
}

/// The Gaussian sigma the satin runs (the typed scratch plan's
/// `blur_sigma`): 0 under the law, `MATTE.split(size, 0).1` otherwise.
pub(crate) fn satin_sigma(size: f32) -> f32 {
    if satin_law_applies(size) {
        0.0
    } else {
        MATTE.split(size, 0.0).1
    }
}

/// The matte `p` of the module doc's algorithm as bytes: the law's at
/// spread 0 ([`satin_law_applies`]), else today's `MATTE` blur.
fn matte_with_job(
    coverage: &[u8],
    w: u32,
    h: u32,
    size: f32,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    if satin_law_applies(size) {
        let m = law_matte_with_job(coverage, w, h, size, 0.0, LawBand::AntiAliased, job)?;
        job.check()?;
        return Ok(m.into_iter().map(law_byte).collect());
    }
    let mut soft = coverage.to_vec();
    job.check()?;
    MATTE.blur_with_job(&mut soft, w, h, satin_sigma(size), job)?;
    Ok(soft)
}

/// Renders `fx` over `shape`: one Interior contribution whose coverage is
/// the (optionally inverted) combine of the two offset copies of the
/// blurred shape, bounded by `shape.coverage` (the Interior rule). Nothing
/// at opacity 0. The context is unused — Satin has no global-light option.
/// Polls `job` before every primitive, inside the blur and once per ROW of
/// the combine (13A-5b-i).
pub(crate) fn render(
    fx: &Satin,
    shape: &Shape,
    _ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Vec<Contribution>, String> {
    if fx.skipped_by_settings() {
        return Ok(Vec::new());
    }
    job.check()?;
    let soft = matte_with_job(&shape.coverage, shape.w, shape.h, fx.size, job)?;
    let (dx, dy) = light_offset(fx.angle, fx.distance);
    job.check()?;
    let mut a = shift_plane(&soft, shape.w, shape.h, dx, dy, 0);
    job.check()?;
    // `saturating_neg`: `light_offset` clamps to the i32 range, and
    // `-i32::MIN` would overflow for a directly constructed absurd distance.
    let mut b = shift_plane(
        &soft,
        shape.w,
        shape.h,
        dx.saturating_neg(),
        dy.saturating_neg(),
        0,
    );
    drop(soft);
    let lut = effect_lut(&fx.contour, fx.anti_aliased, None);
    if let (Some(lut), true) = (&lut, CONTOUR_PER_COPY) {
        map_plane_with_job(&mut a, shape.w, lut, None, job)?;
        map_plane_with_job(&mut b, shape.w, lut, None, job)?;
    }
    // Row by row (`chunks`, so a ragged tail is still visited): the same
    // pairs in the same order as one whole-plane zip.
    let row = (shape.w as usize).max(1);
    let mut plane: Vec<u8> = Vec::with_capacity(a.len().min(b.len()));
    for (ra, rb) in a.chunks(row).zip(b.chunks(row)) {
        job.check()?;
        plane.extend(ra.iter().zip(rb).map(|(&a, &b)| {
            if SATIN_COMBINE_ABS {
                abs_xor(a, b)
            } else {
                soft_xor(a, b)
            }
        }));
    }
    if let (Some(lut), false) = (&lut, CONTOUR_PER_COPY) {
        map_plane_with_job(&mut plane, shape.w, lut, None, job)?;
    }
    if fx.invert {
        job.check()?;
        invert_plane(&mut plane);
    }
    job.check()?;
    multiply_planes(&mut plane, &shape.coverage);
    Ok(vec![Contribution::solid(
        EffectKind::Satin,
        plane,
        slot_color(fx.color, fx.linear),
        fx.blend,
        fx.opacity,
        Placement::Interior,
    )])
}

/// `round(255 (a + b - 2ab))` for coverage bytes `a`, `b` — the soft XOR
/// of the module doc in its exact integer form (the member `abs_xor`
/// replaced, #578).
fn soft_xor(a: u8, b: u8) -> u8 {
    let (a, b) = (u32::from(a), u32::from(b));
    (((a + b) * 255 - 2 * a * b + 127) / 255) as u8
}

/// `|a - b|` for coverage bytes `a`, `b` — the shipped combine (module
/// doc), exact on the bytes.
fn abs_xor(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}
