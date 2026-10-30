//! THE CMYK TIFF sample reader (#547 phase 3): the one decode of a CMYK
//! TIFF's own samples, which the CMYK document open (`cmyk_io`) and the flat
//! open's sRGB conversion (`open_profile::cmyk_tiff_to_srgb8`) both read
//! through. The gate (`open_profile::cmyk_tiff_layout`) has already admitted
//! the file and described it as a [`CmykTiffLayout`].
//!
//! * **Layout.** Chunky (PlanarConfiguration 1, the four inks interleaved
//!   with any extra samples) or planar (2, one plane per sample), 8 or 16
//!   bits, through `tiff`'s own whole-image readout (`read_image_to_buffer`,
//!   which keeps the colour type's samples — the inks and the transparency —
//!   in either layout and never decodes the samples past them). A planar
//!   file whose strip or tile list is not one set per sample is refused by
//!   name ([`PLANAR_STRIPS`]; `tiff` itself refuses it before any colour
//!   type is known, so [`planar_shortfall`] reads the directory alone).
//! * **Alpha.** The FIRST extra sample is the transparency when its
//!   ExtraSamples value is 1 (associated) or 2 (unassociated) — `tiff`'s own
//!   CMYKA rule. Associated (premultiplied) ink is unpremultiplied at the
//!   file's own depth, `round(v · max / a)` capped at `max`, and a pixel at
//!   alpha 0 is paper (ink 0): it has no recoverable ink. Photoshop's Save
//!   Transparency writes associated alpha (`doctored-fixtures/photoshop/
//!   cmyk-psd-547/take1`, `alpha8.tif`/`alpha16.tif`). Every other extra
//!   sample (a spot or saved alpha channel) is counted, never read.
//! * **Samples** come back as straight CMYKA16 ink: an 8-bit code ×257,
//!   alpha 65535 when the file has none.

use std::io::Cursor;

use tiff::decoder::{Decoder, DecodingResult};

use crate::cmyk_image::CmykSample;

/// How the first extra sample stores the transparency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AlphaKind {
    /// ExtraSamples 1: the inks are premultiplied by alpha.
    Associated,
    /// ExtraSamples 2: straight inks.
    Unassociated,
}

/// A CMYK TIFF as the gate describes it (`open_profile::cmyk_tiff_layout`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CmykTiffLayout {
    pub width: u32,
    pub height: u32,
    /// Bytes a sample: 1 or 2.
    pub bytes: u64,
    /// SamplesPerPixel, 4…56.
    pub samples: u16,
    /// PlanarConfiguration 2.
    pub planar: bool,
    pub alpha: Option<AlphaKind>,
    /// Extra samples past the four inks and the transparency, not read.
    pub extras: u16,
}

/// The refusal of a planar CMYK TIFF without one strip or tile set a sample.
pub(crate) const PLANAR_STRIPS: &str = "a planar CMYK TIFF needs one strip or tile set per sample";

/// True for a separated (Photometric 5) planar TIFF whose strip or tile
/// offsets are not one set per sample — header bytes only, both byte orders,
/// the first directory. `false` for anything it cannot read.
pub(crate) fn planar_shortfall(bytes: &[u8]) -> bool {
    let little = match bytes.get(..4) {
        Some([b'I', b'I', 42, 0]) => true,
        Some([b'M', b'M', 0, 42]) => false,
        _ => return false,
    };
    let word = |at: usize| -> Option<u32> {
        let b = bytes.get(at..at + 2)?;
        Some(u32::from(if little {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }))
    };
    let long = |at: usize| -> Option<u32> {
        let b: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let Some(directory) = long(4).map(|d| d as usize) else {
        return false;
    };
    let Some(count) = word(directory) else {
        return false;
    };
    let (mut photometric, mut planar, mut samples) = (0, 1, 1);
    let (mut height, mut rows, mut strips) = (0, u32::MAX, None);
    let (mut tile, mut tiles, mut width) = ((0, 0), None, 0);
    for i in 0..count.min(4096) as usize {
        let at = directory + 2 + i * 12;
        let (Some(tag), Some(kind), Some(n)) = (word(at), word(at + 2), long(at + 4)) else {
            return false;
        };
        let scalar = if kind == 3 {
            word(at + 8)
        } else {
            long(at + 8)
        };
        match tag {
            256 => width = scalar.unwrap_or(0),
            257 => height = scalar.unwrap_or(0),
            262 => photometric = scalar.unwrap_or(0),
            277 => samples = scalar.unwrap_or(1),
            278 => rows = scalar.unwrap_or(u32::MAX),
            284 => planar = scalar.unwrap_or(1),
            273 => strips = Some(n),
            322 => tile.0 = scalar.unwrap_or(0),
            323 => tile.1 = scalar.unwrap_or(0),
            324 => tiles = Some(n),
            _ => {}
        }
    }
    if photometric != 5 || planar != 2 || height == 0 {
        return false;
    }
    let (chunks, offsets) = match (tiles, tile) {
        (Some(n), (tw, th)) if tw > 0 && th > 0 => (
            u64::from(width.div_ceil(tw)) * u64::from(height.div_ceil(th)),
            n,
        ),
        _ => (
            u64::from(height.div_ceil(rows.clamp(1, height))),
            strips.unwrap_or(0),
        ),
    };
    u64::from(offsets) != chunks * u64::from(samples)
}

impl CmykTiffLayout {
    /// The samples `tiff`'s readout keeps a pixel in either layout: the four
    /// inks and the transparency (module doc).
    fn decoded_samples(&self) -> u64 {
        4 + u64::from(self.alpha.is_some())
    }

    /// The bytes `tiff` decodes the whole image into (its output buffer).
    pub(crate) fn decoded_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * self.decoded_samples() * self.bytes
    }

    /// The decoded bytes past the four inks — the transparency — which an
    /// open's plan charges beside its ink.
    pub(crate) fn extra_decoded_bytes(&self) -> u64 {
        u64::from(self.width)
            * u64::from(self.height)
            * self.decoded_samples().saturating_sub(4)
            * self.bytes
    }
}

/// The file's straight CMYKA16 samples, row-major, in STORED orientation
/// (module doc). `Err` names `path`.
pub(crate) fn read_samples(
    bytes: &[u8],
    path: &str,
    layout: &CmykTiffLayout,
) -> Result<Vec<CmykSample>, String> {
    let fail = |what: String| format!("failed to decode {path}: {what}");
    let decoder = Decoder::new(Cursor::new(bytes)).map_err(|e| fail(e.to_string()))?;
    let mut decoder = decoder.with_limits(crate::open_profile::image_tiff_limits(
        layout.decoded_bytes(),
    ));
    let mut data = if layout.bytes == 1 {
        DecodingResult::U8(Vec::new())
    } else {
        DecodingResult::U16(Vec::new())
    };
    let preference = decoder
        .read_image_to_buffer(&mut data)
        .map_err(|e| fail(e.to_string()))?;
    let count = layout.width as usize * layout.height as usize;
    let stride = layout.decoded_samples() as usize;
    let short = || fail("the image data is shorter than its dimensions".into());
    if data.as_buffer(0).as_bytes().len() < preference.complete_len {
        return Err(short());
    }
    // Sample `c` of pixel `i` at the file's own depth, and that depth's max.
    let (read, max): (Box<dyn Fn(usize, usize) -> u32 + '_>, u32) = match &data {
        DecodingResult::U8(v) => {
            if v.len() < count * stride {
                return Err(short());
            }
            (
                if layout.planar {
                    Box::new(move |c, i| u32::from(v[c * count + i]))
                } else {
                    Box::new(move |c, i| u32::from(v[i * stride + c]))
                },
                255,
            )
        }
        DecodingResult::U16(v) => {
            if v.len() < count * stride {
                return Err(short());
            }
            (
                if layout.planar {
                    Box::new(move |c, i| u32::from(v[c * count + i]))
                } else {
                    Box::new(move |c, i| u32::from(v[i * stride + c]))
                },
                65535,
            )
        }
        _ => return Err(fail("CMYK samples changed representation".into())),
    };
    let scale = 65535 / max;
    let mut out = crate::precision_image::reserved_vec(count)?;
    for i in 0..count {
        let alpha = if layout.alpha.is_some() {
            read(4, i)
        } else {
            max
        };
        let mut sample: CmykSample = [0, 0, 0, 0, (alpha * scale) as u16];
        for (c, slot) in sample.iter_mut().take(4).enumerate() {
            let stored = read(c, i);
            let ink = match layout.alpha {
                Some(AlphaKind::Associated) if alpha == 0 => 0,
                Some(AlphaKind::Associated) if alpha < max => {
                    ((stored * max + alpha / 2) / alpha).min(max)
                }
                _ => stored,
            };
            *slot = (ink * scale) as u16;
        }
        out.push(sample);
    }
    Ok(out)
}
