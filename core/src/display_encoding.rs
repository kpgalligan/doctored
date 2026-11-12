//! The display encoding of a 32-bit float document: the ONE in-range
//! encode and decode ([`encode_unit`], [`decode_unit`]) and everything stated
//! on top of them. Two families read it:
//!
//! - every filter with a DISPLAY-REFERRED parameter
//!   (`float_filter_route::Domain::Display`, and the float Red Eye in
//!   `float_retouch`), through [`DisplayEncoding`]: the filter's own kernel
//!   runs unchanged on an encoded copy of the layer, continued above white,
//!   and its result is decoded back to linear light;
//! - the display-defined tools and adjustments (R1–R5 and A below), which
//!   CLAMP to 0…1 before encoding: a tolerance or a classification compares
//!   what the display shows, and a value above white compares as white.
//!
//! The 32-bit spot-channel overprint (`spot_typed`) reads it as the first
//! family does: the kernel on an encoded copy, continued above white, its
//! result decoded back.
//!
//! A kernel module: free functions and small structs over an encoder, a
//! profile model and plain samples; the adapters (`float_filter_route`,
//! `float_retouch`, `float_paint_tone`, `precision_selection`,
//! `precision_scoped_source`, `adjust_float`) hand them in.
//!
//! `E(v)` per RGB channel `c`, odd in `v` (`E(−v) = −E(v)`):
//!
//! - `0 ≤ v ≤ 1`: the document profile's own forward curve, through the
//!   `PrecisionToneEncoder` the float policy already holds for display and
//!   SDR export ([`encode_unit`]), so a value inside 0…1 lands on the code
//!   its 16-bit twin holds (within the encoder's one-interval bound).
//! - `v > 1`: `1 + s_c · ln v`, with `s_c` the curve's slope at white
//!   (`PrecisionToneEncoder::white_slope`), so the join is C1 and no edge
//!   appears at white.
//!
//! The decode is the profile's `MatrixTrc::decode_rgb` on 0…1
//! ([`decode_unit`]) and `exp((f − 1) / s_c)` above, held inside
//! `±FLOAT_RANGE_MAX` (the stored range is the one clamp, as
//! `retouch_encoding::decode`'s).
//!
//! Why extend the curve rather than scale by the layer's headroom. Every
//! value inside 0…1 keeps the code its 16-bit twin holds, so a threshold, a
//! tolerance or a midpoint means the same picture at both depths, and it
//! never depends on the brightest pixel the layer happens to hold: adding
//! one highlight to a corner does not move the filter's rule anywhere else.
//! Above white the logarithm spends equal code on equal ratios of light, the
//! way a power curve spends it on visible steps below: sRGB's `E(65504)` is
//! about 5.9, so the highlight fixture's 5000 : 0.1 (`float_filters_highlight`)
//! becomes about 13.6 : 1 in code, a step these kernels handle the way they
//! handle an SDR edge, which is why they need no row in that file.
//!
//! [`DisplayEncoding::decode_changed`] decodes only the components the kernel
//! changed, so a component a filter leaves alone keeps its stored bits.
//!
//! This is NOT `retouch_encoding`'s W: that one is sRGB-only and scaled to
//! ±65535 so a patch distance and the membrane's code gates read in 16-bit
//! units; this one follows the document's own profile and stays in 0…1 code
//! for SDR values, which is what a filter's 0…1 parameters are stated in.
//!
//! Arithmetic is binary64 between the f32 ends.
//!
//! # The display-defined rules, verbatim
//!
//! E here is [`encode_unit`] (the forward curve through the policy's own
//! encoder, or for an adjustment one built from the same model) and D is
//! [`decode_unit`]. Neither the tone map nor the view exposure is ever read:
//! a rule must not change with how the document happens to be viewed. Unlike
//! a display-referred filter, no rule continues E above white.
//!
//! - **R1, the display copy.** RGB is `round(65535·E(clamp(v,0,1)))`. Alpha
//!   is `unit_to_u16`. So R1 equals `display_sample` under Clip, bit for bit
//!   ([`display_code`]).
//! - **R2, comparisons.** Wand, Bucket, Grow and Similar compare 16-bit codes
//!   (tolerance ×257) on R1. Color Range and Quick Selection compute their Lab
//!   ΔE76 on R1 codes. Every output is native u16 coverage. A value above
//!   white compares as white.
//!   - **E, the edge stage.** Select and Mask's `refine_edge` (edge radius,
//!     smart radius, the refine categories) reads each pixel's R1 code as its
//!     guide, so a value above white refines as flat white; the output is
//!     native u16 coverage. It covers Select and Mask, Quick Selection's
//!     Enhance Edge and Object Selection's refinement. Object Selection's
//!     refinement is opt-in at float: Vision found the object on the
//!     document's own tone map, and the edge stage on R1 sees an above-white
//!     subject on an above-white background as flat white, so by default it
//!     would rewrite an outline it cannot see.
//!   - **M, the Magnetic Lasso.** At float its trace image is the composite
//!     through R1 at 8 bits (the host's Clip display view at zero exposure),
//!     never the document's tone map or the window's exposure; 8 and 16-bit
//!     documents keep their analysis composite.
//! - **R3, the bucket.** The region is filled in linear light through the
//!   float paint walk, in the float modes. Document ink is decoded; `linear`
//!   ink (`rgba32`, or a foreground above white) is used as given.
//! - **R4, Dodge/Burn on a float layer**, per RGB component v, with
//!   `shaped` = `doc_retouch::shaped` ([`DodgeBurnLinear`]):
//!   - 0 ≤ v ≤ 1: t = D(shaped(E(v)));
//!   - v > 1: t = v·D(shaped(1));
//!   - v < 0: t = v + D(shaped(0)).
//!
//!   Under Protect Tones (#1) the in-range triple is moved as one group:
//!   `doc_retouch::protected` on `E(clamp(v, 0, 1))` replaces `shaped` per
//!   component, then D; a component above white or below zero keeps its
//!   scale or shift above.
//!
//!   The output is `v + c·(t − v)` in f64, rounded once to f32 and held to
//!   ±65504. Alpha-0 pixels are skipped and alpha is never written. Above
//!   white the tool scales the value by what it does to white, below zero it
//!   shifts it by what it does to black, so both sides join the display
//!   range continuously and a highlight keeps its headroom.
//! - **R4m, Dodge/Burn on a mask or alpha channel**, at every native depth:
//!   the sample becomes `65535·shaped(s/65535)`, lerped by stroke ×
//!   selection coverage. A mask is coverage, not light, so nothing is
//!   encoded (`precision_scalar_edit`).
//! - **R5, the Sponge on a float layer.** `retouch_brush::targets_in` at
//!   `SampleRange::Extended` over linear samples, unclamped, held to ±65504
//!   (`float_retouch_brush`).
//! - **A, the display forms of an adjustment** ([`through_display`]), the
//!   master being the 8/16-bit `Adjustment::apply_at` entry:
//!   - **Mapped** (Threshold, Posterize, Gradient Map):
//!     `D(master(E(clamp(v,0,1))))`. Above white classifies as white; the
//!     output is wholly the master's.
//!   - **Scaled** (Vibrance, Selective Color, Replace Color, and the
//!     Hue/Saturation settings with no closed form): with `m = max(1, max
//!     rgb)` and `c = rgb/m`, the output is `m·(D(master(E(max(c,0)))) +
//!     min(c,0))`. A highlight is treated as its colour at m× intensity, so
//!     the form is homogeneous above white: out(k·p) = k·out(p) for k ≥ 1
//!     and max p = 1.
//!
//!   The Develop Filter's stage 7 (Vibrance and Saturation) takes the
//!   Scaled form ([`DisplayEncoding::scaled`]) with the unbound sRGB-curve
//!   `Vibrance` master, on the linear value after the encoded copy is decoded: the form
//!   Image ▸ Adjustments ▸ Vibrance takes at float (rule V).
//!
//! Every form runs in f64 and rounds once to f32.
//!
//! # X, the extended-range display sample (EDR)
//!
//! An EDR canvas shows a float document's values above white brighter than
//! white, so its buffer carries LINEAR light, not codes: no curve, no tone
//! map, and the host wraps the buffer in the document profile's
//! extended-linearized colour space, which does the encode a display needs.
//! Per pixel ([`extended_display_sample`]): each RGB sample times the view
//! exposure's gain, held to ±65504 (binary16's largest finite value), then
//! times alpha (the buffer is premultiplied, as a CoreGraphics bitmap is);
//! each result and alpha are rounded once, to nearest-even, to IEEE binary16.
//! Negatives stay (an extended-range space shows them as out-of-gamut
//! colour). E, D and R1–R5 are untouched by X: it is a view form, read by
//! `float_view::display_extended` only.

use crate::doc_retouch::shaped;
use crate::float_document::FLOAT_RANGE_MAX;
use crate::icc_transform::{MatrixTrc, PrecisionToneEncoder};
use crate::precision_image::unit_to_u16;

/// E on 0…1: the profile's forward curve through `encoder`, each component
/// clamped to 0…1 (the encoder's own clamp). The ONE in-range encode:
/// [`DisplayEncoding::encode`] at or below white and every display-defined
/// rule (R1, R4, A) call it.
pub(crate) fn encode_unit(encoder: &PrecisionToneEncoder, linear: [f64; 3]) -> [f32; 3] {
    encoder.rgb(linear)
}

/// D on 0…1: the profile's own decode (`MatrixTrc::decode_rgb`), in f64.
/// The ONE in-range decode: [`DisplayEncoding::decode`] at or below white
/// and every display-defined rule (R4, A) call it.
pub(crate) fn decode_unit(model: &MatrixTrc, code: [f32; 3]) -> [f64; 3] {
    model.decode_rgb(code)
}

/// The encode/decode pair of one document profile (module doc).
pub(crate) struct DisplayEncoding<'a> {
    model: &'a MatrixTrc,
    encoder: &'a PrecisionToneEncoder,
    slope: [f64; 3],
}

impl<'a> DisplayEncoding<'a> {
    /// `Err` when a channel's curve has no finite positive slope at white:
    /// the logarithm above white would have no scale.
    pub(crate) fn new(
        model: &'a MatrixTrc,
        encoder: &'a PrecisionToneEncoder,
    ) -> Result<Self, String> {
        let slope = encoder.white_slope();
        if slope.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
            return Err(
                "the document profile's tone curve is flat at white, so it has no display encoding above white"
                    .into(),
            );
        }
        Ok(Self {
            model,
            encoder,
            slope,
        })
    }

    /// One straight linear sample with its RGB encoded; alpha kept.
    pub(crate) fn encode(&self, linear: [f32; 4]) -> [f32; 4] {
        let magnitude: [f64; 3] = std::array::from_fn(|c| f64::from(linear[c].abs()));
        let curve = encode_unit(self.encoder, magnitude);
        let mut out = linear;
        for c in 0..3 {
            let code = if magnitude[c] <= 1.0 {
                f64::from(curve[c])
            } else {
                1.0 + self.slope[c] * magnitude[c].ln()
            };
            out[c] = code.copysign(f64::from(linear[c])) as f32;
        }
        out
    }

    /// One encoded sample as linear light, RGB inside `±FLOAT_RANGE_MAX`
    /// whatever the code; alpha kept.
    pub(crate) fn decode(&self, code: [f32; 4]) -> [f32; 4] {
        let magnitude: [f64; 3] = std::array::from_fn(|c| f64::from(code[c].abs()));
        let curve = decode_unit(self.model, magnitude.map(|f| f.min(1.0) as f32));
        let mut out = code;
        for c in 0..3 {
            let linear = if magnitude[c] <= 1.0 {
                curve[c]
            } else {
                ((magnitude[c] - 1.0) / self.slope[c]).exp()
            };
            out[c] = linear
                .min(f64::from(FLOAT_RANGE_MAX))
                .copysign(f64::from(code[c])) as f32;
        }
        out
    }

    /// A linear triple through `master` under the Scaled form (module doc,
    /// A), with this encoding's encoder and decode.
    pub(crate) fn scaled(&self, rgb: [f32; 3], master: impl Fn([f32; 3]) -> [f32; 3]) -> [f32; 3] {
        through_display(DisplayRule::Scaled, rgb, self.encoder, self.model, master)
    }

    /// The write of one pixel: `result` is the kernel's answer for `code`,
    /// the encoding of `linear`. A component whose result is bitwise its
    /// code keeps `linear`'s bits; any other is decoded. Alpha is the
    /// result's.
    pub(crate) fn decode_changed(
        &self,
        linear: [f32; 4],
        code: [f32; 4],
        result: [f32; 4],
    ) -> [f32; 4] {
        let decoded = self.decode(result);
        let mut out = decoded;
        for c in 0..3 {
            if result[c].to_bits() == code[c].to_bits() {
                out[c] = linear[c];
            }
        }
        out[3] = result[3];
        out
    }
}

/// R1: one straight float sample as its display code. RGB through E after a
/// clamp to 0…1 (`ToneMap::Clip`'s rule, so a value above white compares as
/// white), then the one `unit_to_u16`; alpha through `unit_to_u16`. Bit for
/// bit what `FloatPolicy::display_sample` under Clip quantizes to.
pub(crate) fn display_code(encoder: &PrecisionToneEncoder, pixel: [f32; 4]) -> [u16; 4] {
    let rgb = encode_unit(
        encoder,
        [pixel[0], pixel[1], pixel[2]].map(|v| f64::from(v.clamp(0.0, 1.0))),
    );
    [
        unit_to_u16(rgb[0]),
        unit_to_u16(rgb[1]),
        unit_to_u16(rgb[2]),
        unit_to_u16(pixel[3]),
    ]
}

/// X: one straight float sample as its extended-range display sample
/// (module doc, X): `clamp(rgb · gain, ±65504) · alpha` and alpha, each as
/// binary16 bits rounded to nearest-even.
pub fn extended_display_sample(pixel: [f32; 4], gain: f32) -> [u16; 4] {
    let alpha = pixel[3];
    let rgb =
        |v: f32| half::f16::from_f32((v * gain).clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX) * alpha);
    [
        rgb(pixel[0]).to_bits(),
        rgb(pixel[1]).to_bits(),
        rgb(pixel[2]).to_bits(),
        half::f16::from_f32(alpha).to_bits(),
    ]
}

/// R4: Dodge or Burn on a float layer's linear samples (module doc).
pub(crate) struct DodgeBurnLinear {
    exposure: f32,
    range: u8,
    burn: bool,
    protect_tones: bool,
    encoder: PrecisionToneEncoder,
    model: MatrixTrc,
    /// `D(shaped(1))` per channel: the factor above white.
    white: [f64; 3],
    /// `D(shaped(0))` per channel: the shift below zero.
    black: [f64; 3],
}

impl DodgeBurnLinear {
    /// The rule at `exposure` (0…1) over `range` (0 shadows, 1 midtones,
    /// 2 highlights), dodging or burning, through the document's own curves:
    /// `encoder` is E and `model`'s decode D; `protect_tones` is the module
    /// doc's group rule.
    pub(crate) fn new(
        exposure: f32,
        range: u8,
        burn: bool,
        protect_tones: bool,
        encoder: PrecisionToneEncoder,
        model: MatrixTrc,
    ) -> Self {
        let white = decode_unit(&model, [shaped(1.0, exposure, range, burn); 3]);
        let black = decode_unit(&model, [shaped(0.0, exposure, range, burn); 3]);
        Self {
            exposure,
            range,
            burn,
            protect_tones,
            encoder,
            model,
            white,
            black,
        }
    }

    /// The fully exposed target `t` of each RGB component (module doc, R4),
    /// in f64 so the caller's lerp rounds once.
    pub(crate) fn target(&self, rgb: [f32; 3]) -> [f64; 3] {
        let shaped_triple = if self.protect_tones {
            let mut group = encode_unit(&self.encoder, rgb.map(|v| f64::from(v).clamp(0.0, 1.0)));
            crate::doc_retouch::protected(&mut group, self.exposure, self.range, self.burn);
            group
        } else {
            let encoded = encode_unit(&self.encoder, rgb.map(f64::from));
            encoded.map(|v| shaped(v, self.exposure, self.range, self.burn))
        };
        let toned = decode_unit(&self.model, shaped_triple);
        std::array::from_fn(|c| {
            let v = f64::from(rgb[c]);
            if v > 1.0 {
                v * self.white[c]
            } else if v < 0.0 {
                v + self.black[c]
            } else {
                toned[c]
            }
        })
    }

    /// One straight pixel under stroke × selection `coverage` (0…1]:
    /// `v + c·(t − v)` per RGB component in f64, rounded once to f32 and
    /// held to ±65504; alpha kept. An alpha-0 pixel is returned as it is.
    pub(crate) fn mixed(&self, pixel: [f32; 4], coverage: f64) -> [f32; 4] {
        if pixel[3] == 0.0 {
            return pixel;
        }
        let target = self.target([pixel[0], pixel[1], pixel[2]]);
        let mut out = pixel;
        for c in 0..3 {
            let v = f64::from(pixel[c]);
            out[c] =
                ((v + coverage * (target[c] - v)) as f32).clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX);
        }
        out
    }
}

/// Which display form an adjustment takes (module doc, A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisplayRule {
    /// `D(master(E(clamp(v,0,1))))`.
    Mapped,
    /// `m·(D(master(E(max(c,0)))) + min(c,0))`, `m = max(1, max rgb)`,
    /// `c = rgb/m`.
    Scaled,
}

/// A linear triple through `master`, a rule defined on encoded numbers in
/// 0…1, under `rule` (module doc, A): `encoder` is E and `model`'s decode D.
/// In f64, rounded once to f32.
pub(crate) fn through_display(
    rule: DisplayRule,
    rgb: [f32; 3],
    encoder: &PrecisionToneEncoder,
    model: &MatrixTrc,
    master: impl Fn([f32; 3]) -> [f32; 3],
) -> [f32; 3] {
    match rule {
        DisplayRule::Mapped => {
            let encoded = encode_unit(encoder, rgb.map(|v| f64::from(v).clamp(0.0, 1.0)));
            decode_unit(model, master(encoded)).map(|v| v as f32)
        }
        DisplayRule::Scaled => {
            let m = rgb.iter().fold(1.0_f64, |m, &v| m.max(f64::from(v)));
            let c = rgb.map(|v| f64::from(v) / m);
            let encoded = encode_unit(encoder, c.map(|v| v.max(0.0)));
            let decoded = decode_unit(model, master(encoded));
            std::array::from_fn(|k| (m * (decoded[k] + c[k].min(0.0))) as f32)
        }
    }
}
