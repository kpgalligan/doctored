//! The ink of a native paint request in a 32-bit float document: what
//! `float_paint`'s walk composites at each pixel, as straight LINEAR RGBA in
//! the document's primaries.
//!
//! [`FloatInk::Solid`] is `paint_blend`'s one colour (Fill, Stroke, the
//! Brush) and `bucket_fill`'s. [`FloatInk::Ramp`] is `gradient`'s: every stop is converted to
//! linear light FIRST ([`linear_ink`]) and the ramp is then interpolated
//! between those linear colours, so a black → white gradient reads `t` in the
//! stored samples and an end above white is reached in a straight line. The
//! field is `gradient_field`'s (`styled_parameter`, `stop_color`), the one
//! the 8- and 16-bit gradients walk, at the request's Smoothness
//! (`gradient_field::smooth_color`, 1 when absent); a two-colour request is
//! the two stops `start` at 0 and `end` at 1 over `radial`'s shape. `erase_native` has no
//! ink (`Ok(None)`): the walk lowers alpha and reads no colour; nor has
//! `dodge_burn`, whose pixels take `display_encoding`'s R4 instead.
//!
//! This is an adapter, not a kernel module: the ink is decoded through the
//! document's own profile.

use crate::gradient_field::{stop_color, styled_parameter, FieldStop, FieldStyle};
use crate::icc::IccProfile;
use crate::icc_transform::{LinearInk, PrecisionTransform};
use crate::pattern_tile::PatternSampler;
use crate::precision_document::PrecisionDocument;
use crate::precision_paint::PaintColorSpace;
use crate::precision_paint_tools::{pattern_sampler, NativePaintEdit};

/// What a float paint walk composites (module doc).
pub(crate) enum FloatInk {
    /// One straight linear RGBA colour at every pixel.
    Solid([f32; 4]),
    /// A stops ramp of straight linear RGBA colours over the drag
    /// `from → to` in `style`'s shape at the Smoothness `smoothness` (a
    /// fraction, `gradient_field::stop_color`'s `k`).
    Ramp {
        stops: Vec<FieldStop>,
        from: [f64; 2],
        to: [f64; 2],
        style: FieldStyle,
        smoothness: f32,
    },
    /// Edit ▸ Fill…'s Pattern contents: the tile on the canvas grid
    /// (`precision_paint_tools::pattern_sampler`), each texel's code / 255
    /// read as linear light — an `rgb32` tile's own values, light above white
    /// kept (`PatternSampler::linear`) — as a 32-bit Pattern Overlay reads it
    /// (a Gray document's tile greyed by the 32-bit rule first).
    Pattern(PatternSampler),
}

impl FloatInk {
    /// The ink of `edit` in `doc`: `PaintBlend` and `BucketFill` are
    /// [`Self::Solid`];
    /// `Gradient` is [`Self::Ramp`] (no stops: `start` at 0 and `end` at 1 in
    /// `radial`'s shape; stops: theirs, in `style` or `radial`'s shape), every
    /// colour through [`linear_ink`]. `EraseNative` and `DodgeBurn` have none
    /// (`Ok(None)`).
    pub(crate) fn of(
        edit: &NativePaintEdit,
        doc: &PrecisionDocument,
    ) -> Result<Option<Self>, String> {
        match edit {
            NativePaintEdit::PaintBlend {
                pattern: Some(pattern),
                ..
            } => Ok(Some(Self::Pattern(pattern_sampler(pattern, doc)?.linear()))),
            NativePaintEdit::PaintBlend {
                color, color_space, ..
            }
            | NativePaintEdit::BucketFill {
                color, color_space, ..
            } => Ok(Some(Self::Solid(linear_ink(*color, *color_space, doc)?))),
            NativePaintEdit::Gradient {
                start,
                end,
                color_space,
                from,
                to,
                radial,
                stops,
                style,
                smoothness,
                ..
            } => {
                let shape = if *radial {
                    FieldStyle::Radial
                } else {
                    FieldStyle::Linear
                };
                let (authored, style) = if stops.is_empty() {
                    let ends = vec![
                        FieldStop {
                            position: 0.0,
                            color: *start,
                            midpoint: 0.5,
                            opacity_midpoint: 0.5,
                        },
                        FieldStop {
                            position: 1.0,
                            color: *end,
                            midpoint: 0.5,
                            opacity_midpoint: 0.5,
                        },
                    ];
                    (ends, shape)
                } else {
                    (stops.clone(), style.unwrap_or(shape))
                };
                let colors: Vec<[f32; 4]> = authored.iter().map(|stop| stop.color).collect();
                let stops = authored
                    .iter()
                    .zip(linear_inks(&colors, *color_space, doc)?)
                    .map(|(stop, color)| FieldStop {
                        position: stop.position,
                        color,
                        midpoint: stop.midpoint,
                        opacity_midpoint: stop.opacity_midpoint,
                    })
                    .collect();
                Ok(Some(Self::Ramp {
                    stops,
                    from: *from,
                    to: *to,
                    style,
                    smoothness: *smoothness,
                }))
            }
            NativePaintEdit::EraseNative { .. } | NativePaintEdit::DodgeBurn { .. } => Ok(None),
        }
    }

    /// The straight linear RGBA at the canvas `point` (a pixel centre).
    pub(crate) fn at(&self, point: [f64; 2]) -> [f32; 4] {
        match self {
            Self::Solid(color) => *color,
            Self::Ramp {
                stops,
                from,
                to,
                style,
                smoothness,
            } => stop_color(
                stops,
                styled_parameter(point, *from, *to, *style) as f32,
                *style,
                *smoothness,
            ),
            // The pixel whose centre `point` is.
            Self::Pattern(sampler) => sampler.at(point[0].floor() as i64, point[1].floor() as i64),
        }
    }
}

/// The authored ink as LINEAR light in the document's primaries
/// (`float_paint`'s module doc); alpha passes through, and so does linear
/// ink, whole. The one-colour form of [`linear_inks`].
pub(crate) fn linear_ink(
    color: [f32; 4],
    space: PaintColorSpace,
    doc: &PrecisionDocument,
) -> Result<[f32; 4], String> {
    Ok(linear_inks(&[color], space, doc)?[0])
}

/// Every one of `colors` as [`linear_ink`] answers it, through ONE decoder
/// or transform built for the call: a gradient's stops share it instead of
/// each building its own.
pub(crate) fn linear_inks(
    colors: &[[f32; 4]],
    space: PaintColorSpace,
    doc: &PrecisionDocument,
) -> Result<Vec<[f32; 4]>, String> {
    let inker = LinearInker::new(space, doc)?;
    colors.iter().map(|&color| inker.linear(color)).collect()
}

/// [`linear_inks`]' decoder or transform built once and applied one colour
/// at a time: a `paint_blend` with ink planes decodes every pixel's ink
/// through it (`float_paint`).
pub(crate) struct LinearInker<'a> {
    doc: &'a PrecisionDocument,
    decode: Decode,
}

/// How [`LinearInker`] reaches linear light.
enum Decode {
    /// Linear ink, used as is.
    Linear,
    /// The document's own encoded components through its curves.
    Document(LinearInk),
    /// sRGB through its curves and the sRGB → document matrix.
    Srgb(PrecisionTransform),
}

impl<'a> LinearInker<'a> {
    /// The decoder of `space` into `doc` ([`linear_inks`]' rule): a profile
    /// without a matrix/TRC model refuses every space but `linear`.
    pub(crate) fn new(space: PaintColorSpace, doc: &'a PrecisionDocument) -> Result<Self, String> {
        let decode = if matches!(space, PaintColorSpace::Linear) {
            Decode::Linear
        } else {
            let destination = doc
                .profile()
                .model()
                .ok_or("native paint requires RGB matrix/TRC profile")?;
            match space {
                PaintColorSpace::Document => Decode::Document(LinearInk::decoding(destination)),
                PaintColorSpace::Srgb => {
                    let source = IccProfile::srgb();
                    Decode::Srgb(
                        PrecisionTransform::new(
                            source.model().ok_or("missing sRGB model")?,
                            destination,
                        )
                        .map_err(|e| {
                            crate::icc_encode_refusal::own_profile_error(doc.profile(), e)
                        })?,
                    )
                }
                PaintColorSpace::Linear => Decode::Linear,
            }
        };
        Ok(Self { doc, decode })
    }

    /// One authored colour as linear light; alpha passes through. A Gray
    /// document greys it by Mode ▸ Grayscale's rule (`gray_document`,
    /// "Luminance"): linear `Y`, so its storage stays R = G = B.
    pub(crate) fn linear(&self, color: [f32; 4]) -> Result<[f32; 4], String> {
        let rgb = [color[0], color[1], color[2]];
        let ink = match &self.decode {
            Decode::Linear => color,
            Decode::Document(ink) => {
                let linear = ink.linear(rgb);
                [
                    linear[0] as f32,
                    linear[1] as f32,
                    linear[2] as f32,
                    color[3],
                ]
            }
            Decode::Srgb(transform) => {
                let linear = transform.linear(rgb);
                [
                    linear[0] as f32,
                    linear[1] as f32,
                    linear[2] as f32,
                    color[3],
                ]
            }
        };
        if !self.doc.is_gray() {
            return Ok(ink);
        }
        let rgb = [ink[0], ink[1], ink[2]].map(f64::from);
        let y = crate::gray_document::GrayPolicy::linear_luminance(self.doc.profile(), rgb)? as f32;
        Ok([y, y, y, ink[3]])
    }
}
