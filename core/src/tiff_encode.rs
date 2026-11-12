//! Typed TIFF encoding with document density and standard capture tags.
//! Byte and high-depth writers share the same metadata/tag implementation.
//! All layout/offset generation belongs to the TIFF crate, including child
//! EXIF directories; metadata_tiff owns the shared relocation algorithm.
//!
//! The typed encoder owns layout and marks RGBA8 as straight alpha. A CMYK
//! document's save (#1, `cmyk_io`) writes four-sample CMYK8/CMYK16 through
//! the same tags with no ExtraSamples: its fourth sample is K, not alpha.
//! A Grayscale document's export (#1 phase 1b, `gray_encode`) writes one
//! BlackIsZero sample, or two with the second tagged straight alpha.
//! A Lab Color document's save (#1 phase 1b, `lab_save`) writes three
//! CIELab samples, or four with the fourth tagged straight alpha.
//! An Indexed Color document's export (#1 Indexed phase 1b,
//! `indexed_encode`) writes one 8-bit RGB Palette sample with its ColorMap,
//! or two with the second tagged straight alpha.
//! A Bitmap document's export (#1 Bitmap phase 1c, `bitmap_export`)
//! writes its packed 1-bit plane BlackIsZero ([`encode_bilevel`]); every
//! writer here shares the sidecar's tags (`side_tags`).
//! An RGB or CMYK file whose sidecar carries spot channels (#1 1c-2a,
//! `tiff_spot`) writes them as trailing extra samples ([`encode_with_spots`]),
//! with their resources after the 8BIM run in tag 34377.

use image::RgbaImage;
use std::io::{Seek, Write};
use tiff::encoder::{colortype, DirectoryEncoder, Rational, TiffEncoder, TiffKindStandard};
use tiff::tags::{PhotometricInterpretation, SampleFormat, Tag};

use crate::metadata_tiff::{export_exif, write_children, write_values, Node};
use crate::metadata_write::EncodeSidecar;

/// A flat TIFF's spot channels as the encoder writes them (#1 1c-2a,
/// `tiff_spot`): the 8BIM resources describing them (after the inherited
/// run in tag 34377), the ExtraSamples of every sample after the colour
/// samples (the transparency's first when the file keeps one), and each
/// spot plane's STORED samples at the file's depth, row-major, canvas-sized.
pub(crate) struct TiffSpotPlanes {
    pub resources: Vec<u8>,
    pub extra_samples: Vec<u16>,
    pub planes: Vec<Vec<u16>>,
}

/// The most bytes one strip of a spot-channel file holds, so the
/// interleaved copy is bounded rather than a second image.
pub(crate) const SPOT_STRIP_BYTES: u64 = 8 << 20;

/// Interleaved input samples for [`encode_with_spots`]: four a pixel (RGBA
/// or CMYK) at 8 or 16 bits.
enum Samples<'a> {
    Eight(&'a [u8]),
    Sixteen(&'a [u16]),
}

impl Samples<'_> {
    fn len(&self) -> usize {
        match self {
            Samples::Eight(s) => s.len(),
            Samples::Sixteen(s) => s.len(),
        }
    }

    fn at(&self, i: usize) -> u16 {
        match self {
            Samples::Eight(s) => u16::from(s[i]),
            Samples::Sixteen(s) => s[i],
        }
    }
}

pub(crate) fn encode<W: Write + Seek>(
    pixels: &RgbaImage,
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if let Some(spots) = &side.spots {
        let samples = Samples::Eight(pixels.as_raw());
        return encode_with_spots(samples, 3, 2, pixels.dimensions(), writer, side, spots);
    }
    encode_samples::<_, colortype::RGBA8>(
        pixels.as_raw(),
        pixels.dimensions(),
        writer,
        side,
        true,
        None,
    )
}

pub(crate) fn encode_rgba16<W: Write + Seek>(
    samples: &[u16],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if let Some(spots) = &side.spots {
        let samples = Samples::Sixteen(samples);
        return encode_with_spots(samples, 3, 2, dimensions, writer, side, spots);
    }
    encode_samples::<_, colortype::RGBA16>(samples, dimensions, writer, side, true, None)
}

/// Interleaved 8-bit CMYK ink (0 = none), four samples a pixel.
pub(crate) fn encode_cmyk8<W: Write + Seek>(
    samples: &[u8],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if let Some(spots) = &side.spots {
        let samples = Samples::Eight(samples);
        return encode_with_spots(samples, 4, 5, dimensions, writer, side, spots);
    }
    encode_samples::<_, colortype::CMYK8>(samples, dimensions, writer, side, false, None)
}

/// Interleaved 16-bit CMYK ink (0 = none), four samples a pixel.
pub(crate) fn encode_cmyk16<W: Write + Seek>(
    samples: &[u16],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if let Some(spots) = &side.spots {
        let samples = Samples::Sixteen(samples);
        return encode_with_spots(samples, 4, 5, dimensions, writer, side, spots);
    }
    encode_samples::<_, colortype::CMYK16>(samples, dimensions, writer, side, false, None)
}

/// An RGB (`colour` 3, Photometric 2) or CMYK (4, Photometric 5) file of
/// four-sample `samples` with `spots` as trailing extra samples (module
/// doc): the colour samples, the input's fourth sample as straight alpha
/// only when `spots` declares one more extra sample than it has planes (an
/// RGBA input's alpha; never a CMYK one's K), then each spot plane.
/// Uncompressed chunky strips of at most [`SPOT_STRIP_BYTES`], written
/// through `TiffEncoder::image_directory` as [`encode_bilevel`] is, since
/// the crate's image encoder knows no colour type of this many samples.
fn encode_with_spots<W: Write + Seek>(
    samples: Samples<'_>,
    colour: usize,
    photometric: u16,
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
    spots: &TiffSpotPlanes,
) -> Result<(), String> {
    let fail = |what: &str| format!("encoding TIFF failed: {what}");
    let (width, height) = dimensions;
    let pixels = width as usize * height as usize;
    let alpha = colour == 3 && spots.extra_samples.len() == spots.planes.len() + 1;
    let count = colour + usize::from(alpha) + spots.planes.len();
    if samples.len() != pixels * 4 {
        return Err(fail("the samples do not match the dimensions"));
    }
    if spots.extra_samples.len() != count - colour {
        return Err(fail("the extra samples do not match the spot channels"));
    }
    if spots.planes.iter().any(|plane| plane.len() != pixels) {
        return Err(fail("a spot channel plane does not match the dimensions"));
    }
    let sixteen = matches!(samples, Samples::Sixteen(_));
    let sample_bytes = if sixteen { 2 } else { 1 };
    let row_bytes = (u64::from(width) * (count * sample_bytes) as u64).max(1);
    let rows = u32::try_from((SPOT_STRIP_BYTES / row_bytes).max(1))
        .unwrap_or(u32::MAX)
        .min(height.max(1));
    let interleaved = |y0: u32, y1: u32| -> Vec<u16> {
        let mut out = Vec::with_capacity((y1 - y0) as usize * width as usize * count);
        for p in y0 as usize * width as usize..y1 as usize * width as usize {
            for c in 0..colour {
                out.push(samples.at(p * 4 + c));
            }
            if alpha {
                out.push(samples.at(p * 4 + 3));
            }
            for plane in &spots.planes {
                out.push(plane[p]);
            }
        }
        out
    };
    let run = || -> tiff::TiffResult<()> {
        let mut encoder = TiffEncoder::new(writer)?;
        let root = side.exif.as_deref().and_then(export_exif);
        let children = match &root {
            Some(root) => write_children(&mut encoder, root)?,
            None => Vec::new(),
        };
        let mut tags = encoder.image_directory()?;
        side_tags(&mut tags, side, root.as_ref(), &children, None)?;
        let mut offsets = Vec::new();
        let mut counts = Vec::new();
        let mut y = 0;
        while y < height {
            let end = y.saturating_add(rows).min(height);
            let strip = interleaved(y, end);
            let offset = if sixteen {
                tags.write_data(strip.as_slice())?
            } else {
                let bytes: Vec<u8> = strip.iter().map(|&v| v as u8).collect();
                tags.write_data(bytes.as_slice())?
            };
            offsets.push(u32::try_from(offset)?);
            counts.push(u32::try_from(strip.len() * sample_bytes)?);
            y = end;
        }
        tags.write_tag(Tag::ImageWidth, width)?;
        tags.write_tag(Tag::ImageLength, height)?;
        tags.write_tag(
            Tag::BitsPerSample,
            &vec![8 * sample_bytes as u16; count][..],
        )?;
        tags.write_tag(Tag::Compression, 1u16)?;
        tags.write_tag(Tag::PhotometricInterpretation, photometric)?;
        tags.write_tag(Tag::SamplesPerPixel, count as u16)?;
        tags.write_tag(Tag::RowsPerStrip, rows)?;
        tags.write_tag(Tag::PlanarConfiguration, 1u16)?;
        tags.write_tag(Tag::ExtraSamples, spots.extra_samples.as_slice())?;
        tags.write_tag(Tag::SampleFormat, &vec![1u16; count][..])?;
        tags.write_tag(Tag::StripOffsets, offsets.as_slice())?;
        tags.write_tag(Tag::StripByteCounts, counts.as_slice())?;
        tags.finish()
    };
    run().map_err(|e| format!("encoding TIFF failed: {e}"))
}

/// Gray plus straight alpha, two unsigned samples a pixel (BlackIsZero);
/// `encode_samples` never asks for a predictor, so none is implemented (the
/// `tiff_float::half_color_type!` pattern).
macro_rules! gray_alpha_color_type {
    ($name:ident, $inner:ty, $bits:expr) => {
        struct $name;
        impl colortype::ColorType for $name {
            type Inner = $inner;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::BlackIsZero;
            const BITS_PER_SAMPLE: &'static [u16] = &[$bits; 2];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::Uint; 2];
            fn horizontal_predict(_: &[$inner], _: &mut Vec<$inner>) {
                unreachable!()
            }
        }
    };
}
gray_alpha_color_type!(GrayAlpha8, u8, 8);
gray_alpha_color_type!(GrayAlpha16, u16, 16);

/// Interleaved 8-bit gray (0 = black), one sample a pixel, or two with
/// `alpha` (the second straight alpha).
pub(crate) fn encode_gray8<W: Write + Seek>(
    samples: &[u8],
    alpha: bool,
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if alpha {
        encode_samples::<_, GrayAlpha8>(samples, dimensions, writer, side, true, None)
    } else {
        encode_samples::<_, colortype::Gray8>(samples, dimensions, writer, side, false, None)
    }
}

/// [`encode_gray8`] at 16 bits.
pub(crate) fn encode_gray16<W: Write + Seek>(
    samples: &[u16],
    alpha: bool,
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if alpha {
        encode_samples::<_, GrayAlpha16>(samples, dimensions, writer, side, true, None)
    } else {
        encode_samples::<_, colortype::Gray16>(samples, dimensions, writer, side, false, None)
    }
}

/// TIFF 6.0 CIELab, three samples (four with straight alpha), unsigned on
/// every sample as `lab_encode` lays a\*/b\*'s signed bits into them, and no
/// predictor (the `gray_alpha_color_type!` pattern).
macro_rules! cielab_color_type {
    ($name:ident, $inner:ty, $bits:expr, $count:expr) => {
        struct $name;
        impl colortype::ColorType for $name {
            type Inner = $inner;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::CIELab;
            const BITS_PER_SAMPLE: &'static [u16] = &[$bits; $count];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::Uint; $count];
            fn horizontal_predict(_: &[$inner], _: &mut Vec<$inner>) {
                unreachable!()
            }
        }
    };
}
cielab_color_type!(CieLab8, u8, 8, 3);
cielab_color_type!(CieLabAlpha8, u8, 8, 4);
cielab_color_type!(CieLab16, u16, 16, 3);
cielab_color_type!(CieLabAlpha16, u16, 16, 4);

/// Interleaved 8-bit CIELab samples (`lab_encode::tiff_samples8`), three a
/// pixel, or four with `alpha` (the fourth straight alpha).
pub(crate) fn encode_cielab8<W: Write + Seek>(
    samples: &[u8],
    alpha: bool,
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if alpha {
        encode_samples::<_, CieLabAlpha8>(samples, dimensions, writer, side, true, None)
    } else {
        encode_samples::<_, CieLab8>(samples, dimensions, writer, side, false, None)
    }
}

/// [`encode_cielab8`] at 16 bits (`lab_encode::tiff_samples16`).
pub(crate) fn encode_cielab16<W: Write + Seek>(
    samples: &[u16],
    alpha: bool,
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if alpha {
        encode_samples::<_, CieLabAlpha16>(samples, dimensions, writer, side, true, None)
    } else {
        encode_samples::<_, CieLab16>(samples, dimensions, writer, side, false, None)
    }
}

/// One 8-bit RGB Palette index (and straight alpha), unsigned, no
/// predictor (the `gray_alpha_color_type!` pattern).
macro_rules! palette_color_type {
    ($name:ident, $count:expr) => {
        struct $name;
        impl colortype::ColorType for $name {
            type Inner = u8;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::RGBPalette;
            const BITS_PER_SAMPLE: &'static [u16] = &[8; $count];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::Uint; $count];
            fn horizontal_predict(_: &[u8], _: &mut Vec<u8>) {
                unreachable!()
            }
        }
    };
}
palette_color_type!(Palette8, 1);
palette_color_type!(PaletteAlpha8, 2);

/// Interleaved 8-bit palette indices, one sample a pixel, or two with
/// `alpha` (the second straight alpha), and the ColorMap (320): 3 × 256
/// SHORTs, every red, then every green, then every blue.
pub(crate) fn encode_palette8<W: Write + Seek>(
    samples: &[u8],
    alpha: bool,
    color_map: &[u16],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    if alpha {
        encode_samples::<_, PaletteAlpha8>(samples, dimensions, writer, side, true, Some(color_map))
    } else {
        encode_samples::<_, Palette8>(samples, dimensions, writer, side, false, Some(color_map))
    }
}

fn encode_samples<W: Write + Seek, C: colortype::ColorType>(
    samples: &[C::Inner],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
    straight_alpha: bool,
    color_map: Option<&[u16]>,
) -> Result<(), String>
where
    [C::Inner]: tiff::encoder::TiffValue,
{
    let run = || -> tiff::TiffResult<()> {
        let mut encoder = TiffEncoder::new(writer)?;
        let root = side.exif.as_deref().and_then(export_exif);
        let children = match &root {
            Some(root) => write_children(&mut encoder, root)?,
            None => Vec::new(),
        };
        let mut image = encoder.new_image::<C>(dimensions.0, dimensions.1)?;
        side_tags(image.encoder(), side, root.as_ref(), &children, color_map)?;
        // RGBA already counts its fourth sample (gray + alpha its second).
        // ImageEncoder::extra_samples would add one more; tag the existing
        // sample as straight alpha here.
        if straight_alpha {
            image.encoder().write_tag(Tag::ExtraSamples, &[2u16][..])?;
        }
        image.write_data(samples)
    };
    run().map_err(|e| format!("encoding TIFF failed: {e}"))
}

/// The sidecar's tags every writer here shares, in one fixed order (each
/// out-of-line value lands in the file as it is written): the EXIF root's
/// values, the ColorMap, the ICC, XMP and IPTC, the resolution and the
/// orientation.
fn side_tags<W: Write + Seek>(
    tags: &mut DirectoryEncoder<'_, W, TiffKindStandard>,
    side: &EncodeSidecar<'_>,
    root: Option<&Node>,
    children: &[(u16, u32)],
    color_map: Option<&[u16]>,
) -> tiff::TiffResult<()> {
    if let Some(root) = root {
        write_values(tags, root, children)?;
    }
    if let Some(map) = color_map {
        tags.write_tag(Tag::ColorMap, map)?;
    }
    if let Some(icc) = side.icc {
        tags.write_tag(Tag::Unknown(34675), icc)?;
    }
    if let Some(xmp) = &side.xmp {
        tags.write_tag(Tag::Unknown(700), xmp.as_slice())?;
    }
    // The paths' resources (`tiff_paths`), then the spot channels', follow
    // the inherited (filtered) run.
    let spot_resources = side
        .spots
        .as_ref()
        .map_or(&[][..], |spots| spots.resources.as_slice());
    let inherited = side.iptc.as_deref().unwrap_or_default();
    if !inherited.is_empty() || !side.paths.is_empty() || !spot_resources.is_empty() {
        let run = [inherited, side.paths.as_slice(), spot_resources].concat();
        tags.write_tag(Tag::Unknown(34377), run.as_slice())?;
    }
    // Use the fallible tag setters: ImageEncoder's resolution helpers
    // internally unwrap I/O errors, which would lose the path diagnostic.
    let dpi = side.dpi.unwrap_or_default().sane();
    let rational = |ppi: f32| Rational {
        n: (f64::from(ppi) * 10_000.0).round() as u32,
        d: 10_000,
    };
    tags.write_tag(Tag::XResolution, rational(dpi.x))?;
    tags.write_tag(Tag::YResolution, rational(dpi.y))?;
    tags.write_tag(Tag::ResolutionUnit, 2u16)?;
    tags.write_tag(Tag::Orientation, 1u16)
}

/// A Bitmap document's kept TIFF (#1 Bitmap phase 1c, `bitmap_export`):
/// the packed plane (`bitmap_method`: `ceil(w/8)` bytes a row, MSB first, 1
/// white) as BitsPerSample 1, Photometric 1 (BlackIsZero, so a set bit is
/// white and the rows are written as they stand), one uncompressed strip,
/// with the sidecar's tags. Written through `TiffEncoder::image_directory`,
/// because the crate's image encoder assumes whole-byte samples.
pub(crate) fn encode_bilevel<W: Write + Seek>(
    packed: &[u8],
    dimensions: (u32, u32),
    writer: &mut W,
    side: &EncodeSidecar<'_>,
) -> Result<(), String> {
    let (width, height) = dimensions;
    let size = (width as usize).div_ceil(8) as u64 * u64::from(height);
    if packed.len() as u64 != size {
        return Err("encoding TIFF failed: the Bitmap plane does not match the dimensions".into());
    }
    let count = u32::try_from(size)
        .map_err(|_| "encoding TIFF failed: the strip is too large".to_string())?;
    let run = || -> tiff::TiffResult<()> {
        let mut encoder = TiffEncoder::new(writer)?;
        let root = side.exif.as_deref().and_then(export_exif);
        let children = match &root {
            Some(root) => write_children(&mut encoder, root)?,
            None => Vec::new(),
        };
        let mut tags = encoder.image_directory()?;
        side_tags(&mut tags, side, root.as_ref(), &children, None)?;
        tags.write_tag(Tag::ImageWidth, width)?;
        tags.write_tag(Tag::ImageLength, height)?;
        tags.write_tag(Tag::BitsPerSample, 1u16)?;
        tags.write_tag(Tag::Compression, 1u16)?;
        tags.write_tag(Tag::PhotometricInterpretation, 1u16)?;
        tags.write_tag(Tag::SamplesPerPixel, 1u16)?;
        tags.write_tag(Tag::RowsPerStrip, height.max(1))?;
        tags.write_tag(Tag::PlanarConfiguration, 1u16)?;
        let offset = tags.write_data(packed)?;
        let offset = u32::try_from(offset)?;
        tags.write_tag(Tag::StripOffsets, offset)?;
        tags.write_tag(Tag::StripByteCounts, count)?;
        tags.finish()
    };
    run().map_err(|e| format!("encoding TIFF failed: {e}"))
}
