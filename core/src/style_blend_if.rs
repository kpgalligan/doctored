//! Blend If (Layer Style > Blending Options > Blend If): a
//! per-pixel weight in `[0, 1]` that `style_composite` multiplies into a
//! layer pixel's alpha before it blends — the photographer's "this warm
//! adjustment only in the highlights", and the cheapest of the blending
//! options (no plane, no blur: two ramp lookups per pixel).
//!
//! **The ramp.** A split-slider pair `[lo0, lo1, hi0, hi1]` over the 8-bit
//! range, `lo0 <= lo1 <= hi0 <= hi1` (the parser enforces the order), maps
//! a channel value `v` to a weight:
//!
//! | `v`                | weight                    |
//! |--------------------|---------------------------|
//! | `v < lo0`          | 0                         |
//! | `lo0 <= v < lo1`   | `(v - lo0) / (lo1 - lo0)` |
//! | `lo1 <= v <= hi0`  | 1                         |
//! | `hi0 < v <= hi1`   | `(hi1 - v) / (hi1 - hi0)` |
//! | `v > hi1`          | 0                         |
//!
//! A JOINED pair is a hard edge: `lo0 == lo1` weights 1 from `lo0` up
//! (the second row is empty, so no division happens), `hi0 == hi1` weights
//! 0 above `hi0`. Splitting a handle with ⌥-drag in the dialog opens the
//! ramp rows, which softens the transition. The branch
//! guards make the two divisions safe by construction — each denominator is
//! strictly positive whenever its row is reached — and every row lands in
//! `[0, 1]`, so even a ramp handed in out of order (nothing produces one,
//! but the function must not care) yields a valid weight.
//!
//! **The channel value.** Ramps are defined over 8-bit values, so the
//! straight `0..1` colour is scaled to `0..=255` and ROUNDED to the nearest
//! integer: the source pixel is 8-bit already and the rounding only undoes
//! the `/255 * 255` float error that would otherwise put a pixel of exactly
//! `lo0` on the wrong side of a hard edge; the backdrop is an f32 composite,
//! and rounding evaluates it as the 8-bit pixel it becomes when the
//! projection quantizes, so a hard edge at 100 splits the same pixels
//! Photoshop's does. Gray is the luminosity `0.30 r + 0.59 g + 0.11 b` of
//! the triple as stored (`blend::lum`, the W3C weights' one home) — fitted to
//! Photoshop 26.10.0 (`scripts/photoshop-probe/style_v1x_take.py`):
//! exact on the A4 hard Underlying row at 8 bits and within 0.05 code on every
//! 16- and 32-bit receipt row, where the Rec. 709 luma missed by up to 116 and
//! 191 codes on chromatic pixels. Red / green / blue read one channel. The
//! value is clamped to `[0, 255]` after scaling because the three
//! coefficients sum to 1 only in decimal — in f32 a pure white pixel can come
//! out a hair above 255, which a full ramp `[0, 0, 255, 255]` would otherwise
//! weight 0.
//!
//! **Two ramps, one weight.** `source_weight` = this-layer ramp on the
//! layer pixel's own colour × underlying ramp on the composite BENEATH the
//! layer. "Underlying" is the accumulator pixel as stored at the moment the
//! layer starts drawing — the snapshot `style_composite` takes before the
//! layer's own Below effects, because the Underlying Layer is what lies
//! under the layer, and the layer's own drop shadow is not under it.
//! The snapshot's alpha is ignored: the ramps are functions of colour, and
//! the accumulator keeps straight colour, so what a partially covered
//! backdrop pixel looks like IS its stored colour. A never-touched pixel is
//! the accumulator's cleared value, transparent black, and so evaluates as
//! 0 — the deterministic answer where there is no colour to read (a layer
//! floating over nothing is the case Blend If is least meant for).
//!
//! **Several channels at once**. A Blend If may hold one
//! range per channel (Gray plus Red / Green / Blue on RGB, Gray plus the
//! four plates on CMYK); each range's weight is the two-ramp weight above
//! on its own channel, and the layer's weight is their PRODUCT (`product`,
//! every `source_weight*`). Phase 1's take recorded PRODUCT as the closest
//! of PRODUCT and MIN (`scripts/photoshop-probe/styles_scope_558_take.py`,
//! M1 / M2); `scripts/photoshop-probe/blend_if_multichannel_take.py` judges
//! it on soft ranges from Photoshop's own single-range rows. A single range
//! is the shipped arithmetic statement for statement.
//!
//! `style_blend_if::source_weight_16` is the typed compositor's twin of `source_weight`: the ONE
//! master `ramp_weight_at` table and the ONE `channel_value` reader with the knees at `257·k` and
//! the scale at 65535, so the byte arithmetic is bit-identical and the typed weight is the same
//! rational for promoted codes.
//!
//! **At 32-bit float** ([`source_weight_float`]) the ramps read the STORED linear
//! value — measured in Photoshop 26.10.0 before it was built
//! (`scripts/photoshop-probe/float_styles.py`, receipt
//! `doctored-fixtures/photoshop/float-styles-473/take5`, every patch of 16 cases at 1.2e-7):
//! a This Layer edge at 100 splits linear 0.3 (76.5) from 0.42 (107.1), where the encoded values
//! (149, 173) would both be past it. The value is neither rounded nor clamped (−0.25 falls below
//! a black knee at 0; 2.0 rises past a white knee at 255), a FULL `[0, 0, 255, 255]` ramp weights
//! every value 1 (2.0 under an untouched Underlying range shows), and each ramp's ZERO end sits
//! one 15-bit code (1/32768) further out than the knee — the split ramp `[.., 60, 140]` spans
//! `80/255 + 1/32768`, which fits every split-ramp patch where `80/255` misses by up to 1.3e-4.

use crate::blend::lum;
use crate::style::{BlendIf, BlendIfChannel, BlendIfRange, FULL_RAMP};

/// Weight in `[0, 1]` multiplied into the layer pixel's alpha: the
/// this-layer ramp evaluated on `source`'s channel × the underlying ramp on
/// `backdrop`'s (module doc). `source` is the layer pixel's straight RGB in
/// `0..1`; `backdrop` the accumulator pixel beneath the layer as stored
/// (straight RGB + alpha; the alpha is ignored — module doc).
pub(crate) fn source_weight(blend_if: &BlendIf, source: [f32; 3], backdrop: [f32; 4]) -> f32 {
    product(blend_if, |range| {
        let this = ramp_weight(
            channel_value(range.channel, source, 255.0),
            range.this_layer,
        );
        let under = ramp_weight(
            channel_value(
                range.channel,
                [backdrop[0], backdrop[1], backdrop[2]],
                255.0,
            ),
            range.underlying,
        );
        this * under
    })
}

/// The COMBINE rule (module doc): the product of `weight` over every range
/// of `blend_if`, the primary first. A single-range value returns the
/// primary's weight itself, so every single-channel result is today's
/// arithmetic statement for statement.
fn product(blend_if: &BlendIf, weight: impl Fn(BlendIfRange) -> f32) -> f32 {
    let mut out = weight(blend_if.primary());
    for range in blend_if.extra.iter().flatten() {
        out *= weight(*range);
    }
    out
}

/// The typed compositor's twin of [`source_weight`] (`precision_render`):
/// the SAME two ramps evaluated on 16-bit CODES. An 8-bit knee `k` maps to
/// the code `257 * k` — the `precision_conversion` promotion, exact in f32 —
/// and each side's channel value is the straight `0..1` colour scaled to
/// `0..=65535` and rounded, which recovers every stored code exactly for a
/// raster's own pixel and reads the composite beneath as the code the
/// projection would store (the byte module doc's rule, one depth up). Both
/// ramps and both channel values go through the one master table and the
/// one channel reader, so the arithmetic is the byte compositor's with the
/// knees and the scale substituted; for a document whose codes are all
/// promotions the red/green/blue weights are bit-identical to the byte ones
/// (same correctly rounded quotient of the same rational). Gray keeps the
/// sub-8-bit luminosity, so its rounding may land on the other side of a
/// knee where the byte value would not — which agrees with Photoshop's
/// 16-bit Gray (measured: within 0.05 code). `backdrop` is the
/// accumulator's straight RGB; a never-touched pixel reads 0 like the byte
/// path. The result multiplies the layer's effective alpha AFTER
/// `checked_alpha`.
pub(crate) fn source_weight_16(blend_if: &BlendIf, source: [f32; 3], backdrop: [f32; 3]) -> f32 {
    let knees = |ramp: [u8; 4]| ramp.map(|k| f32::from(k) * 257.0);
    product(blend_if, |range| {
        let this = ramp_weight_at(
            channel_value(range.channel, source, 65535.0),
            knees(range.this_layer),
        );
        let under = ramp_weight_at(
            channel_value(range.channel, backdrop, 65535.0),
            knees(range.underlying),
        );
        this * under
    })
}

/// The 32-bit float document's weight (module doc): each side's ramp on the
/// stored linear channel value `× 65535`, unrounded and unclamped, through
/// the master [`ramp_weight_at`] with the knees at `257·k` and the zero ends
/// (`lo0`, `hi1`) moved out by [`FLOAT_KNEE_EXTENSION`]; a full ramp weights
/// every value 1. `source` is the layer's stored straight RGB, `backdrop`
/// the accumulator's (linear light at float).
pub(crate) fn source_weight_float(blend_if: &BlendIf, source: [f32; 3], backdrop: [f32; 3]) -> f32 {
    product(blend_if, |range| {
        float_side(range.channel, source, range.this_layer)
            * float_side(range.channel, backdrop, range.underlying)
    })
}

/// One 15-bit code in the 65535 scale: how far the 32-bit ramps reach past
/// a zero-end knee (measured against Photoshop, module doc).
const FLOAT_KNEE_EXTENSION: f32 = 65535.0 / 32768.0;

fn float_side(channel: BlendIfChannel, rgb: [f32; 3], ramp: [u8; 4]) -> f32 {
    if ramp == FULL_RAMP {
        return 1.0;
    }
    let [lo0, lo1, hi0, hi1] = ramp.map(|k| f32::from(k) * 257.0);
    ramp_weight_at(
        channel_unit(channel, rgb) * 65535.0,
        [
            lo0 - FLOAT_KNEE_EXTENSION,
            lo1,
            hi0,
            hi1 + FLOAT_KNEE_EXTENSION,
        ],
    )
}

/// One split ramp on a channel value `v` in `0..=255` (the table in the
/// module doc). Total over every finite `v` and every ramp, joined or not,
/// ordered or not: the result is always in `[0, 1]` and no division by zero
/// can be reached. A non-finite `v` fails every comparison and falls to the
/// last row, weight 0.
pub(crate) fn ramp_weight(v: f32, ramp: [u8; 4]) -> f32 {
    ramp_weight_at(v, ramp.map(f32::from))
}

/// The master ramp table over already-scaled knees: the byte path hands in
/// `ramp.map(f32::from)` (knees on `0..=255`), the typed path `257 * k`
/// (knees on `0..=65535`). Every branch guard and both divisions are
/// exactly [`ramp_weight`]'s, so the byte result is bit-identical.
pub(crate) fn ramp_weight_at(v: f32, knees: [f32; 4]) -> f32 {
    let [lo0, lo1, hi0, hi1] = knees;
    if v < lo0 {
        0.0
    } else if v < lo1 {
        // Reached only with lo0 <= v < lo1, so lo1 - lo0 > 0.
        (v - lo0) / (lo1 - lo0)
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        // Reached only with hi0 < v <= hi1, so hi1 - hi0 > 0.
        (hi1 - v) / (hi1 - hi0)
    } else {
        0.0
    }
}

/// The integer channel value the ramps read from a straight `0..1` RGB
/// triple: gray is the `blend::lum` luminosity, the others one channel; scaled
/// by `scale` (255 for the 8-bit ramps, 65535 for the typed twin), rounded
/// to the integer the pixel is (or becomes), clamped to `[0, scale]`
/// (module doc).
fn channel_value(channel: BlendIfChannel, rgb: [f32; 3], scale: f32) -> f32 {
    (channel_unit(channel, rgb) * scale)
        .round()
        .clamp(0.0, scale)
}

/// The channel a ramp reads, unscaled: gray is the `blend::lum` luminosity
/// (0.30, 0.59, 0.11, the fitted Gray — module doc), the
/// others one component. TOTAL: a CMYK plate on an RGB pixel (a document a
/// reader admitted one into, which every setter refuses — #547) reads as
/// Gray, so no reader's input can panic or read out of range.
fn channel_unit(channel: BlendIfChannel, rgb: [f32; 3]) -> f32 {
    match channel {
        BlendIfChannel::Gray
        | BlendIfChannel::Cyan
        | BlendIfChannel::Magenta
        | BlendIfChannel::Yellow
        | BlendIfChannel::Black => lum(rgb),
        BlendIfChannel::Red => rgb[0],
        BlendIfChannel::Green => rgb[1],
        BlendIfChannel::Blue => rgb[2],
    }
}

/// A CMYK document's weight (#547), FITTED to Photoshop 26.10.0 on take1 and
/// confirmed on take2 (`scripts/photoshop-probe/cmyk_styles_take.py`, THE
/// BLEND IF FIT; `cmyk-styles-547/take1`, `take2`). Both sides first round
/// each ink to the code the projection stores at the document's depth
/// (`round(code_max·ink)`: the layer's own pixel recovers its code, the
/// Underlying f32 composite becomes the pixel it is stored as, the RGB byte
/// path's rule). A plate reads `code_max − code` (P: Photoshop's slider runs
/// on the stored `L = 1 − ink`, 0 is 100 % ink); Gray reads [`channel_value`]
/// (`blend::lum`, the W3C luminosity) of the codes' naive RGB `(1 − C)(1 −
/// K), (1 − M)(1 − K), (1 − Y)(1 − K)` (G-LUM). Each ramp goes through the
/// master [`ramp_weight_at`] with its knees moved by [`ink_knee_shift`]: INC
/// `[lo0 − 1, lo1, hi0, hi1 + 1]` for Gray, Cyan, Magenta and Black, HALF
/// `[lo0 − ½, lo1 − ½, hi0 + ½, hi1 + ½]` for Yellow (the fit's declared
/// tie-break: on take1 INC and HALF both met the bound on every plate, worst
/// max 1 code; the knees as declared missed by 3 codes on the ramp's slope,
/// where the luma `ink_tone` and the ink domain missed by 103 and 255). At 8
/// bits (`code_max` 255) that is the fitted rule exactly. At 16 bits
/// (`code_max` 65535) it is EXTRAPOLATED, not measured: the value reads the
/// 16-bit code with no 8-bit rounding (Gray is `round(65535·lum)`, the
/// measured 16-bit Gray of [`source_weight_16`]) and the knees and their
/// shifts scale by 257. `source` is the layer's straight ink, `backdrop`
/// the accumulator's (alpha ignored, as at every depth). A Red, Green or
/// Blue ramp (refused on a CMYK document) reads the naive RGB's channel,
/// so the function is total.
pub(crate) fn source_weight_ink(
    blend_if: &BlendIf,
    source: [f32; 4],
    backdrop: [f32; 4],
    code_max: f32,
) -> f32 {
    let scale = code_max / 255.0;
    product(blend_if, |range| {
        let shift = ink_knee_shift(range.channel);
        let knees = |ramp: [u8; 4]| [0, 1, 2, 3].map(|i| (f32::from(ramp[i]) + shift[i]) * scale);
        ramp_weight_at(
            ink_channel_value(range.channel, source, code_max),
            knees(range.this_layer),
        ) * ramp_weight_at(
            ink_channel_value(range.channel, backdrop, code_max),
            knees(range.underlying),
        )
    })
}

/// The fitted knee offsets `[lo0, lo1, hi0, hi1]` in 8-bit units
/// ([`source_weight_ink`]): HALF on Yellow, INC on every other channel.
fn ink_knee_shift(channel: BlendIfChannel) -> [f32; 4] {
    match channel {
        BlendIfChannel::Yellow => [-0.5, -0.5, 0.5, 0.5],
        BlendIfChannel::Gray
        | BlendIfChannel::Cyan
        | BlendIfChannel::Magenta
        | BlendIfChannel::Black
        | BlendIfChannel::Red
        | BlendIfChannel::Green
        | BlendIfChannel::Blue => [-1.0, 0.0, 0.0, 1.0],
    }
}

/// The `0..=code_max` value a CMYK ramp reads from straight ink
/// ([`source_weight_ink`]): each ink first rounded to its stored code, then a
/// plate's `code_max − code`, else [`channel_value`]'s statement on the
/// codes' naive RGB (Gray's `blend::lum`).
fn ink_channel_value(channel: BlendIfChannel, ink: [f32; 4], code_max: f32) -> f32 {
    let code = |v: f32| (v.clamp(0.0, 1.0) * code_max).round();
    let plate = match channel {
        BlendIfChannel::Cyan => Some(0),
        BlendIfChannel::Magenta => Some(1),
        BlendIfChannel::Yellow => Some(2),
        BlendIfChannel::Black => Some(3),
        BlendIfChannel::Gray
        | BlendIfChannel::Red
        | BlendIfChannel::Green
        | BlendIfChannel::Blue => None,
    };
    match plate {
        Some(p) => code_max - code(ink[p]),
        None => {
            let q = ink.map(|v| code(v) / code_max);
            let paper = 1.0 - q[3];
            channel_value(
                channel,
                [
                    (1.0 - q[0]) * paper,
                    (1.0 - q[1]) * paper,
                    (1.0 - q[2]) * paper,
                ],
                code_max,
            )
        }
    }
}
