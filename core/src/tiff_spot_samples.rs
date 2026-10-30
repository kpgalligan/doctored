//! KERNEL (#547 spot channels 1c-2a): a flat TIFF's spot channel samples —
//! the layout test, the read of the saved extra samples and their polarity.
//! `tiff_spot` adapts it to the resources and the two document types;
//! nothing here names a document.
//!
//! The layout is Photoshop 26.10.0's own, measured by
//! `scripts/photoshop-probe/spot_tiff_take.py` take2 (five flat files: RGB at
//! 8 and 16 bits, one with a transparency, and CMYK at 8 and 16 bits):
//!
//! - A spot channel is an EXTRA SAMPLE of the one image: chunky
//!   (PlanarConfiguration 1), its ExtraSamples value 0 (unspecified), the
//!   saved channels the TRAILING samples after the colour samples and the
//!   transparency (PLACE held on every file). Photoshop tags its
//!   transparency 1 (associated); this writer tags its own 2, the straight
//!   alpha every Doctored TIFF declares, and the reader takes either.
//! - Their names and inks are the PSD resources 1006/1045/1077/1053 in tag
//!   34377 (`psd_spot_records`), leading with "Transparency" when the file
//!   has one (LEAD L1).
//! - THE POLARITY ([`tiff_stored`]): H1 on every file, RGB and CMYK — the
//!   plane is stored `max − coverage`, the PSD's rule, even though a CMYK
//!   TIFF stores its own inks as they are. One function reads and writes it.
//!
//! The samples are read by laying a Photometric of 1 (BlackIsZero) over the
//! directory through `tiff_directory::Overlay` (the Lab and palette
//! pattern), which makes the `tiff` decoder hand back every sample of a
//! pixel as one Multiband image in each of its lossless compressions; the
//! file is never copied.

use tiff::decoder::{Decoder, DecodingResult, Limits};

use crate::psd_framing::MAX_PSD_CHANNELS;
use crate::psd_spot_records::stored;
use crate::tiff_directory::{bytes_reader, Directory, Overlay, COMPRESSIONS};

/// The ExtraSamples value this writer gives a spot channel's sample
/// (take2: Photoshop's on every saved channel, "unspecified").
pub(crate) const SPOT_EXTRA_SAMPLE: u16 = 0;
/// The ExtraSamples value of the transparency this writer writes: straight
/// (unassociated) alpha, every Doctored TIFF's.
pub(crate) const TRANSPARENCY_EXTRA_SAMPLE: u16 = 2;

const IMAGE_WIDTH: u16 = 256;
const IMAGE_LENGTH: u16 = 257;
const BITS_PER_SAMPLE: u16 = 258;
const COMPRESSION: u16 = 259;
const PHOTOMETRIC: u16 = 262;
const ORIENTATION: u16 = 274;
const SAMPLES_PER_PIXEL: u16 = 277;
const PLANAR: u16 = 284;
const INK_SET: u16 = 332;
const EXTRA_SAMPLES: u16 = 338;
const SAMPLE_FORMAT: u16 = 339;

/// A flat RGB or CMYK TIFF's samples as the spot reader sees them (module
/// doc): `colour` 3 or 4, `bytes` 1 or 2 a sample, `samples` a pixel, the
/// first ExtraSamples value when it is a transparency (1 or 2), whether the
/// samples are planar, whether the image is upright (Orientation absent or
/// 1), and whether its compression is one the decoder reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TiffSpotLayout {
    pub width: u32,
    pub height: u32,
    pub colour: usize,
    pub bytes: usize,
    pub samples: usize,
    pub transparency: Option<u16>,
    pub planar: bool,
    pub upright: bool,
    pub compression_read: bool,
}

impl TiffSpotLayout {
    /// The first directory's layout: `None` for anything but a classic TIFF
    /// in RGB (Photometric 2) or CMYK (5, InkSet absent or 1) with more
    /// samples than colour samples, all 8 or all 16 bits, unsigned.
    pub(crate) fn of(bytes: &[u8]) -> Option<Self> {
        let mut read = bytes_reader(bytes);
        let directory = Directory::first(&mut read)?;
        let width = directory.scalar(IMAGE_WIDTH)?;
        let height = directory.scalar(IMAGE_LENGTH)?;
        let colour = match directory.scalar(PHOTOMETRIC)? {
            2 => 3,
            5 if directory.entry(INK_SET).is_none() || directory.scalar(INK_SET) == Some(1) => 4,
            _ => return None,
        };
        let samples = directory.scalar(SAMPLES_PER_PIXEL)? as usize;
        if width == 0 || height == 0 || samples <= colour || samples > MAX_PSD_CHANNELS {
            return None;
        }
        let bits: Vec<u16> = directory
            .shorts(BITS_PER_SAMPLE, samples as u32, &mut read)??
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        let bytes = match bits.first() {
            Some(8) => 1,
            Some(16) => 2,
            _ => return None,
        };
        if bits.iter().any(|&b| b != bits[0]) || !(bits.len() == 1 || bits.len() == samples) {
            return None;
        }
        if let Some(formats) = directory.shorts(SAMPLE_FORMAT, samples as u32, &mut read)? {
            if formats.iter().any(|&(f, _)| f != 1) {
                return None;
            }
        }
        let planar = match directory.scalar(PLANAR) {
            None | Some(1) => false,
            Some(2) => true,
            _ => return None,
        };
        let extras = directory.shorts(EXTRA_SAMPLES, samples as u32, &mut read)?;
        let transparency = extras
            .and_then(|v| v.first().map(|&(e, _)| e))
            .filter(|&e| e == 1 || e == 2);
        let compression = directory.scalar(COMPRESSION).unwrap_or(1);
        Some(TiffSpotLayout {
            width,
            height,
            colour,
            bytes,
            samples,
            transparency,
            planar,
            upright: matches!(directory.scalar(ORIENTATION), None | Some(1)),
            compression_read: COMPRESSIONS.contains(&compression),
        })
    }

    /// The first saved channel's sample: after the colour samples and the
    /// transparency (PLACE).
    pub(crate) fn first_saved(&self) -> usize {
        self.colour + usize::from(self.transparency.is_some())
    }

    /// The saved channels: every sample after [`Self::first_saved`].
    pub(crate) fn saved(&self) -> usize {
        self.samples.saturating_sub(self.first_saved())
    }

    /// The bytes [`read_saved_planes`]' decode holds: every sample of every
    /// pixel at the file's depth.
    pub(crate) fn read_bytes(&self) -> u64 {
        u64::from(self.width)
            .saturating_mul(u64::from(self.height))
            .saturating_mul((self.samples * self.bytes) as u64)
    }
}

/// THE polarity (module doc), both directions: a coverage sample as a flat
/// TIFF stores it, and a stored sample as coverage. take2 measured H1 in RGB
/// and CMYK alike, so it is the PSD's map (`psd_spot_records::stored`).
pub(crate) fn tiff_stored(coverage: u16, max: u16) -> u16 {
    stored(coverage, max)
}

/// A spot channel's coverage plane as a layered TIFF writer reads it in
/// place (#547 1c-2b, `tiff_layers_write::write_spots`): canvas-sized, row 0
/// top, at the document's own channel depth (a byte document's 8 bits, a
/// typed one's 16).
#[derive(Clone, Copy, Debug)]
pub(crate) enum SpotCoverage<'a> {
    Eight(&'a [u8]),
    Sixteen(&'a [u16]),
}

impl SpotCoverage<'_> {
    /// The plane's sample count.
    pub(crate) fn pixels(&self) -> usize {
        match self {
            SpotCoverage::Eight(v) => v.len(),
            SpotCoverage::Sixteen(v) => v.len(),
        }
    }

    /// Pixel `p` as a TIFF of `depth` (8 or 16) bits stores it, through
    /// [`tiff_stored`]. A 16-bit plane in an 8-bit file narrows the stored
    /// value by `round(v / 257)`, the rule whose master copy is
    /// `psd_export_cmyk::Depth::push`; an 8-bit plane in a 16-bit file widens
    /// its coverage ×257 first, so the polarity is applied at the file's
    /// own maximum.
    pub(crate) fn stored(&self, p: usize, depth: u16) -> u16 {
        match (*self, depth) {
            (SpotCoverage::Eight(v), 8) => tiff_stored(u16::from(v[p]), 255),
            (SpotCoverage::Eight(v), _) => tiff_stored(u16::from(v[p]) * 257, 65535),
            (SpotCoverage::Sixteen(v), 8) => {
                ((u32::from(tiff_stored(v[p], 65535)) + 128) / 257) as u16
            }
            (SpotCoverage::Sixteen(v), _) => tiff_stored(v[p], 65535),
        }
    }
}

/// The saved channels `saved` (indices from [`TiffSpotLayout::first_saved`])
/// as coverage at the file's depth (0..=255 or 0..=65535), row 0 top, the
/// polarity undone. `Err` for a planar layout, a file the decoder refuses,
/// or samples that do not match the layout.
pub(crate) fn read_saved_planes(
    bytes: &[u8],
    layout: &TiffSpotLayout,
    saved: &[usize],
) -> Result<Vec<Vec<u16>>, String> {
    if layout.planar {
        return Err("planar samples are not read".into());
    }
    let mut read = bytes_reader(bytes);
    let directory = Directory::first(&mut read).ok_or("the TIFF directory is not readable")?;
    let photometric = directory
        .entry(PHOTOMETRIC)
        .ok_or("the TIFF has no Photometric")?;
    let patches = vec![(photometric.at, directory.short_bytes(1))];
    let decoder =
        Decoder::new(Overlay::new(bytes, &patches)).map_err(|e| format!("TIFF samples: {e}"))?;
    let mut limits = Limits::default();
    limits.decoding_buffer_size = usize::try_from(layout.read_bytes())
        .map_err(|_| "the TIFF samples are too large".to_string())?
        .max(crate::drdc::MAX_DRDC_BLOB_LEN as usize);
    limits.ifd_value_size = crate::drdc::MAX_DRDC_BLOB_LEN as usize;
    let mut decoder = decoder.with_limits(limits);
    let stored_dimensions = decoder
        .dimensions()
        .map_err(|e| format!("TIFF samples: {e}"))?;
    if stored_dimensions != (layout.width, layout.height) {
        return Err("the TIFF's dimensions disagree".into());
    }
    let pixels = layout.width as usize * layout.height as usize;
    let first = layout.first_saved();
    let raw = decoder
        .read_image()
        .map_err(|e| format!("TIFF samples: {e}"))?;
    let max = match (&raw, layout.bytes) {
        (DecodingResult::U8(_), 1) => 255,
        (DecodingResult::U16(_), 2) => 65535,
        _ => return Err("the TIFF's samples are not the layout's depth".into()),
    };
    let plane_of = |i: usize| match &raw {
        DecodingResult::U8(v) => v.get(i).map(|&s| u16::from(s)),
        DecodingResult::U16(v) => v.get(i).copied(),
        _ => None,
    };
    let mut out = Vec::with_capacity(saved.len());
    for &index in saved {
        let sample = first + index;
        if sample >= layout.samples {
            return Err("the saved channel is out of range".into());
        }
        let mut plane = Vec::with_capacity(pixels);
        for p in 0..pixels {
            let v = plane_of(p * layout.samples + sample)
                .ok_or("the TIFF's samples are shorter than the layout")?;
            plane.push(tiff_stored(v, max));
        }
        out.push(plane);
    }
    Ok(out)
}
