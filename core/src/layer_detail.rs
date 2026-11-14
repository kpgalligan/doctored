//! Kernel: the detail of a frequency-separation pair where Apply Image's
//! stored-code form cannot rebuild the layer, one pixel at a time. The typed
//! `layer_detail` edit (`precision_layer_detail`, the adapter) runs it; an
//! encoded 8- or 16-bit document keeps Apply Image's Subtract at Scale 2 and
//! Offset ½, whose bound is one code (`core/tests/frequency_separation.rs`).
//!
//! # Difference (a 32-bit float document)
//! `high = t − s` per colour component, the float compositor's own Subtract
//! (`blend_float::blend_kind_at(Subtract, true)`, unclamped), and the pair is
//! recombined by Linear Dodge (Add), which the float compositor also runs
//! unclamped (`cb + cs`). Float storage keeps a negative sample, so no pivot
//! is needed: the Linear Light form `low + 2·high − 1` exists only to hold a
//! signed difference inside 0…1, and at 32 bits it would add a halving and a
//! doubling for nothing. `s + (t − s)` is `t` up to two f32 roundings, so the
//! pair rebuilds the layer within `2^-22 · max(|t|, |s|)` per component.
//!
//! # Decoded (16 bits evaluated in linear light)
//! Such a document composites Linear Light on DECODED components, so the
//! detail is computed there: Apply Image's scaled Subtract
//! (`plane_scale_offset::PlaneBlend`, scale 2, offset ½) on the decoded
//! triples, `clamp((lin(t) − lin(s))/2 + ½)`, encoded once through the
//! document's own transfer (`sampling_linear::LinearCodes`). The compositor
//! then rebuilds `lin(s) + 2·lin(high) − 1 = lin(t) + 2e`, where `e` is the
//! detail's own rounding: at most half a stored code times the decode slope
//! at the detail (≤ 2.275 linear codes per stored code under sRGB, 2.2 under
//! a gamma-2.2 curve). The render's own rounding adds half a stored code at
//! the result's slope, so in decoded components the pair is within about
//! 3.4 codes of 65535 for ANY low. In stored codes the error is that times
//! the encode slope at the result: largest at black, where the detail is at
//! most ½ (slope ≤ 1.52), so ≈ 20 codes under sRGB's 12.92 toe and ≈ 510
//! (two 8-bit levels) under a pure power curve, whose encode slope is
//! unbounded at black. An 8-bit document would see the same arithmetic at
//! 257 times the step, about twenty LEVELS in the shadows, which is why the
//! host refuses it there.
//!
//! An alpha-0 pixel of `t` is the caller's to skip; each function keeps
//! `t`'s alpha.

use crate::blend::{BlendKind, BlendMode};
use crate::plane_scale_offset::PlaneBlend;
use crate::sampling_linear::LinearCodes;

/// The float compositor's Subtract, the difference form's operator: the
/// separable function `blend_float::blend_kind_at(Subtract, true)` hands
/// back. `None` would mean the table no longer gives a separable Subtract,
/// which the adapter refuses rather than computing a second copy.
pub(crate) fn float_subtract() -> Option<fn(f32, f32) -> f32> {
    match crate::blend_float::blend_kind_at(BlendMode::Subtract, true) {
        BlendKind::Separable(subtract) => Some(subtract),
        _ => None,
    }
}

/// One float pixel's detail (module doc, `# Difference`): `t − s` per colour
/// component through `subtract` ([`float_subtract`]), `t`'s alpha kept.
pub(crate) fn difference(subtract: fn(f32, f32) -> f32, t: [f32; 4], s: [f32; 4]) -> [f32; 4] {
    [
        subtract(t[0], s[0]),
        subtract(t[1], s[1]),
        subtract(t[2], s[2]),
        t[3],
    ]
}

/// Apply Image's Subtract at scale 2 and offset ½, the decoded form's blend
/// (module doc, `# Decoded`).
pub(crate) fn decoded_blend() -> Result<PlaneBlend, &'static str> {
    PlaneBlend::new(BlendMode::Subtract, 2.0, 0.5)
}

/// One stored pixel's detail in the decoded domain (module doc, `# Decoded`):
/// both colours decoded through `codes`, blended by `blend`
/// ([`decoded_blend`]) at full opacity, and encoded once; `t`'s alpha kept.
pub(crate) fn decoded<C: LinearCodes<u16> + ?Sized>(
    blend: PlaneBlend,
    codes: &C,
    t: [u16; 4],
    s: [u16; 4],
) -> [u16; 4] {
    let detail = blend.triple(
        codes.linear([t[0], t[1], t[2]]),
        codes.linear([s[0], s[1], s[2]]),
        1.0,
        (0, 0),
    );
    let out = codes.codes(detail);
    [out[0], out[1], out[2], t[3]]
}
