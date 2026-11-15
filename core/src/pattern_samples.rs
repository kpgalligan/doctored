//! KERNEL: a pattern tile's NATIVE samples (`designs/pattern-tiles.md`) —
//! the numbers a tile defined from a CMYK, 16-bit or 32-bit float document
//! keeps BESIDE its straight RGBA8 raster (`pattern_tile::PatternTile`).
//!
//! # The kinds
//!
//! | Kind | Texel | Range |
//! |---|---|---|
//! | `rgb16` | R, G, B, A as `u16` | the document's own 16-bit codes |
//! | `rgb32` | R, G, B, A as `f32` | linear light, colour finite within ±65504, alpha 0…1 |
//! | `cmyk16` | C, M, Y, K, A as `u16` | the document's own codes, 65535 = full ink |
//!
//! Row 0 at the top, `width × height` texels, the RGBA8 raster's order. The
//! RGBA8 raster stays on every tile: it is the swatch, and every document
//! that does not store these numbers reads it exactly as before.
//!
//! # Who reads them
//!
//! A document reads samples only of a kind it stores, VERBATIM — they are
//! never converted into another kind:
//!
//! * `rgb16` and `rgb32` build the RGB level ([`TileSamples::rgb_texels`]):
//!   an RGB or Indexed document at 8 or 16 bits reads code / 65535 (an
//!   `rgb32` value clamped to 0…1 by the sampler, as the RGBA8 raster's
//!   code / 255 is), a 32-bit document the value itself, light above white
//!   kept (`pattern_tile::PatternSampler::linear`). A Grayscale document
//!   reads them only while every texel is neutral (`gray_style`).
//! * `cmyk16` builds a CMYK document's ink level ([`TileSamples::ink`]),
//!   each texel's ink verbatim (an 8-bit CMYK document's paint stores codes
//!   off its 8-bit grid too); no RGB reader reads it.
//! * A Lab document, a Bitmap custom pattern and a bevel texture's height
//!   read the RGBA8 raster at every kind.
//!
//! # The codec
//!
//! The JSON's `native.data` (`style_pattern_fill`) is the little-endian bytes
//! of every component in texel order ([`TileSamples::to_le_bytes`],
//! [`TileSamples::from_le_bytes`]); a wrong kind, length or range refuses.
//! The `rgb32` range is a 32-bit float document's own stored range
//! (`float_document::FLOAT_RANGE_MAX`), so every sample a float composite
//! can hand Define Pattern is admitted and nothing larger is.

use crate::float_document::FLOAT_RANGE_MAX;

/// A tile's native samples (module doc), `width × height` texels.
#[derive(Clone, Debug, PartialEq)]
pub enum TileSamples {
    Rgb16(Vec<u16>),
    Rgb32(Vec<f32>),
    Cmyk16(Vec<u16>),
}

/// A sample kind's facts: its JSON name, its components per texel and its
/// bytes per component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    Rgb16,
    Rgb32,
    Cmyk16,
}

impl SampleKind {
    /// The kind named `name` in the JSON; `None` for any other.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "rgb16" => Some(Self::Rgb16),
            "rgb32" => Some(Self::Rgb32),
            "cmyk16" => Some(Self::Cmyk16),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rgb16 => "rgb16",
            Self::Rgb32 => "rgb32",
            Self::Cmyk16 => "cmyk16",
        }
    }

    /// Components per texel, alpha last.
    pub fn channels(self) -> usize {
        match self {
            Self::Rgb16 | Self::Rgb32 => 4,
            Self::Cmyk16 => 5,
        }
    }

    /// Bytes per component in the codec and in memory.
    pub fn width(self) -> usize {
        match self {
            Self::Rgb16 | Self::Cmyk16 => 2,
            Self::Rgb32 => 4,
        }
    }

    /// Decoded bytes per texel.
    pub fn texel_bytes(self) -> u64 {
        (self.channels() * self.width()) as u64
    }
}

impl TileSamples {
    pub fn kind(&self) -> SampleKind {
        match self {
            Self::Rgb16(_) => SampleKind::Rgb16,
            Self::Rgb32(_) => SampleKind::Rgb32,
            Self::Cmyk16(_) => SampleKind::Cmyk16,
        }
    }

    /// The number of components held.
    pub fn len(&self) -> usize {
        match self {
            Self::Rgb16(v) | Self::Cmyk16(v) => v.len(),
            Self::Rgb32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The decoded bytes these samples hold.
    pub fn bytes(&self) -> u64 {
        (self.len() * self.kind().width()) as u64
    }

    /// The address of the held components (the resource inventory's key).
    pub fn address(&self) -> usize {
        match self {
            Self::Rgb16(v) | Self::Cmyk16(v) => v.as_ptr() as usize,
            Self::Rgb32(v) => v.as_ptr() as usize,
        }
    }

    /// Every component, little-endian, in texel order (module doc's codec).
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            Self::Rgb16(v) | Self::Cmyk16(v) => v.iter().flat_map(|c| c.to_le_bytes()).collect(),
            Self::Rgb32(v) => v.iter().flat_map(|c| c.to_le_bytes()).collect(),
        }
    }

    /// `bytes` read back as `kind` for `texels` texels: `Err` names what is
    /// wrong — a length other than `texels × kind.texel_bytes()`, or an
    /// `rgb32` component outside its range (module doc's table).
    pub fn from_le_bytes(
        kind: SampleKind,
        bytes: &[u8],
        texels: usize,
    ) -> Result<Self, &'static str> {
        let expected = texels
            .checked_mul(kind.texel_bytes() as usize)
            .ok_or("length")?;
        if bytes.len() != expected {
            return Err("length");
        }
        let samples = match kind {
            SampleKind::Rgb16 | SampleKind::Cmyk16 => {
                let v = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                if kind == SampleKind::Rgb16 {
                    Self::Rgb16(v)
                } else {
                    Self::Cmyk16(v)
                }
            }
            SampleKind::Rgb32 => {
                let v: Vec<f32> = bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                if !v.chunks_exact(4).all(admits_linear) {
                    return Err("range");
                }
                Self::Rgb32(v)
            }
        };
        Ok(samples)
    }

    /// The RGB level's texels, premultiplied unit R, G, B, A, and whether
    /// they reach beyond 0…1 (`rgb32`: the level is EXTENDED, module doc);
    /// `None` for `cmyk16`, which no RGB reader reads.
    pub fn rgb_texels(&self) -> Option<(Vec<[f32; 4]>, bool)> {
        match self {
            Self::Rgb16(v) => Some((
                v.chunks_exact(4)
                    .map(|t| {
                        let a = f32::from(t[3]) / 65535.0;
                        [
                            f32::from(t[0]) / 65535.0 * a,
                            f32::from(t[1]) / 65535.0 * a,
                            f32::from(t[2]) / 65535.0 * a,
                            a,
                        ]
                    })
                    .collect(),
                false,
            )),
            Self::Rgb32(v) => Some((
                v.chunks_exact(4)
                    .map(|t| [t[0] * t[3], t[1] * t[3], t[2] * t[3], t[3]])
                    .collect(),
                true,
            )),
            Self::Cmyk16(_) => None,
        }
    }

    /// Texel `i` of a `cmyk16` tile: straight unit C, M, Y, K and alpha;
    /// `None` for an RGB kind or past the end.
    pub fn ink(&self, i: usize) -> Option<([f32; 4], f32)> {
        let Self::Cmyk16(v) = self else {
            return None;
        };
        let t = v.get(i * 5..i * 5 + 5)?;
        let unit = |c: u16| f32::from(c) / 65535.0;
        Some(([unit(t[0]), unit(t[1]), unit(t[2]), unit(t[3])], unit(t[4])))
    }

    /// Whether an RGB reader would read a colour here: some `rgb16` or
    /// `rgb32` texel whose R, G and B differ (`gray_style`'s question). A
    /// `cmyk16` tile answers false: an RGB reader never reads it.
    pub fn coloured(&self) -> bool {
        match self {
            Self::Rgb16(v) => v.chunks_exact(4).any(|t| t[0] != t[1] || t[1] != t[2]),
            Self::Rgb32(v) => v.chunks_exact(4).any(|t| t[0] != t[1] || t[1] != t[2]),
            Self::Cmyk16(_) => false,
        }
    }

    /// These samples with texel `i` taken from texel `order[i]`: an exact
    /// turn or flip (`reference_point::turned_tile`, whose index raster comes
    /// from the one pixel implementation, `ops::rotate*` and `ops::flip_*`).
    /// `None` when an index lies past the end.
    pub fn permuted(&self, order: &[usize]) -> Option<Self> {
        fn pick<T: Copy>(v: &[T], order: &[usize], n: usize) -> Option<Vec<T>> {
            let mut out = Vec::with_capacity(order.len() * n);
            for &i in order {
                out.extend_from_slice(v.get(i * n..i * n + n)?);
            }
            Some(out)
        }
        Some(match self {
            Self::Rgb16(v) => Self::Rgb16(pick(v, order, 4)?),
            Self::Rgb32(v) => Self::Rgb32(pick(v, order, 4)?),
            Self::Cmyk16(v) => Self::Cmyk16(pick(v, order, 5)?),
        })
    }

    /// A `cmyk16` tile with each texel's ink mapped by `f` (unit C, M, Y, K
    /// in and out, alpha kept, each output component rounded to the nearest
    /// code); any other kind unchanged. Convert to Profile's leg
    /// (`cmyk_profile::converted_inks`).
    pub fn map_inks(
        &self,
        f: &mut dyn FnMut([u16; 4]) -> Result<[f32; 4], String>,
    ) -> Result<Self, String> {
        let Self::Cmyk16(v) = self else {
            return Ok(self.clone());
        };
        let mut out = Vec::with_capacity(v.len());
        for t in v.chunks_exact(5) {
            let ink = f([t[0], t[1], t[2], t[3]])?;
            for c in ink {
                out.push((f64::from(c).clamp(0.0, 1.0) * 65535.0).round() as u16);
            }
            out.push(t[4]);
        }
        Ok(Self::Cmyk16(out))
    }
}

/// Whether one `rgb32` texel is in range (module doc's table).
fn admits_linear(t: &[f32]) -> bool {
    t.len() == 4
        && t[..3]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= FLOAT_RANGE_MAX)
        && t[3].is_finite()
        && (0.0..=1.0).contains(&t[3])
}
