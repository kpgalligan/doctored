//! #547 spot channels 1c-2a: flat TIFF spot channels in and out, the
//! adapter over the `tiff_spot_samples` kernel (the layout, the samples and
//! their polarity) and the `psd_spot_records` kernel (resources 1006, 1045,
//! 1077 and 1053, which Photoshop stores in a TIFF's tag 34377 exactly as in
//! a PSD; `scripts/photoshop-probe/spot_tiff_take.py` take2).
//!
//! READ ([`tiff_spot_records`]). An RGB or CMYK TIFF at 8 or 16 bits whose
//! saved channels — the extra samples after the colour samples and the
//! transparency — are described by its 8BIM run: those of kind spot with a
//! Lab ink are KEPT, by the PSD reader's own rules and loss sentences
//! (`psd_spot::spot_candidates`; the run's lists lead with "Transparency"
//! when the file has one). Planar samples, an Orientation other than 1 and
//! a compression the decoder does not read are a loss line with nothing
//! kept. Every open site calls [`DrDocument::with_tiff_spots`] or
//! [`PrecisionDocument::with_tiff_spots`]: the byte document keeps 8-bit
//! coverage (a 16-bit file narrowed by `psd_export_cmyk::Depth`'s
//! `round(v / 257)`), a typed one u16 (an 8-bit file ×257), admitted first.
//! A plane that cannot be read is a loss line, never a refused open.
//!
//! WRITE ([`DrDocument::tiff_spot_planes`],
//! [`PrecisionDocument::tiff_spot_planes`]). Every spot channel in channel
//! order becomes a trailing extra sample (`tiff_encode::TiffSpotPlanes`),
//! stored through `tiff_stored`, with its resources after the inherited run
//! in tag 34377; a transparency, when the file keeps one, is the first
//! extra sample and leads every list (L1). Which saves keep them is
//! `psd_export_preflight::spots_written_for`'s `FlatTiff` rule: RGB and
//! CMYK at 8 and 16 bits; any other document refuses with
//! `spot_channel::TIFF_REFUSAL`.
//!
//! LAYERED (#547 1c-2b). Photoshop's layered TIFF (take2's `rgb8_l.tif`,
//! `cmyk16_l.tif`) follows the flat rule: the composite IFD's trailing extra
//! samples (ExtraSamples 0, H1), the lists in tag 34377 led by
//! "Transparency", tag 37724 beside them. So the layered TIFF reads and
//! writes them under `spots_written_for`'s same TIFF rule. Read:
//! [`layered_spot_records`] (the container's own `spot_records`, so the
//! import report names them) and [`DrDocument::with_layered_tiff_spots`] /
//! [`PrecisionDocument::with_layered_tiff_spots`], the flat open's body on
//! the container's file and resources. Write: `layered_tiff_spots` borrows
//! the first `spots_capacity` planes in place for
//! `tiff_layers_write::write_spots`, their resources appended to the image
//! resources, the transparency's entry leading (every layered TIFF has one).

use std::sync::Arc;

use image::GrayImage;

use crate::doc::DrDocument;
use crate::doc_channel::{channels_fit_at, Channel};
use crate::doc_export::ExportRequest;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, PrecisionImage};
use crate::precision_mask::{PrecisionChannel, PrecisionPlane};
use crate::psd_container::{resource_in, Container};
use crate::psd_export_cmyk::Depth;
use crate::psd_export_preflight::{spots_capacity, spots_written_for, SpotFile};
use crate::psd_spot::{channel_limit_loss, native_name, spot_candidates, SpotRecords};
use crate::psd_spot_records::encode_resources;
use crate::spot_channel::{SpotInk, TIFF_REFUSAL};
use crate::tiff_encode::{TiffSpotPlanes, SPOT_STRIP_BYTES};
use crate::tiff_spot_samples::{
    read_saved_planes, tiff_stored, SpotCoverage, TiffSpotLayout, SPOT_EXTRA_SAMPLE,
    TRANSPARENCY_EXTRA_SAMPLE,
};

/// A typed export request that keeps spot channels, on a CMYK document.
pub(crate) const CMYK_KEEP_REFUSAL: &str = "export request spot_channels \"keep\" is not available for a CMYK document; its TIFF save keeps the spot channels";

/// The import report's lines are bounded by the typed document's own cap
/// (`PrecisionDocument::with_import_warnings`).
const MAX_IMPORT_WARNINGS: usize = 64;

/// The file's layout and its saved channels' reading (module doc): `None`
/// when the bytes are not an RGB or CMYK TIFF with saved channels described
/// by a DisplayInfo in `run` (the file's 8BIM run, `metadata::scan`'s
/// `iptc`). No sample is decoded.
pub(crate) fn tiff_spot_records(
    bytes: &[u8],
    run: Option<&[u8]>,
) -> Option<(TiffSpotLayout, SpotRecords)> {
    let layout = TiffSpotLayout::of(bytes)?;
    let count = layout.saved();
    let run = run?;
    if count == 0 {
        return None;
    }
    resource_in(run, crate::psd_spot_records::DISPLAY_INFO)
        .ok()
        .flatten()?;
    let mut out = SpotRecords {
        kept: Vec::new(),
        alpha: count,
        losses: Vec::new(),
    };
    // LEAD L1 (take2): with a transparency, every list leads with its entry.
    let candidates = spot_candidates(
        |id| resource_in(run, id).ok().flatten(),
        count,
        layout.transparency.is_some(),
        (layout.width, layout.height),
        &mut out,
    );
    if candidates.is_empty() {
        return Some((layout, out));
    }
    let reason = if layout.planar {
        Some("its samples are planar".to_string())
    } else if !layout.upright {
        let orientation =
            crate::metadata_tiff::orientation_of_file(bytes).map_or(0, |o| o.to_exif());
        Some(format!("it is stored rotated (Orientation {orientation})"))
    } else if !layout.compression_read {
        Some("its compression is not one Doctored reads".to_string())
    } else {
        None
    };
    if let Some(reason) = reason {
        out.losses.push(format!(
            "Spot channels in this TIFF are not read: {reason}."
        ));
        return Some((layout, out));
    }
    out.alpha = count - candidates.len();
    out.kept = candidates;
    Some((layout, out))
}

/// True when the file's every extra sample past its transparency is a kept
/// spot channel ([`tiff_spot_records`]): the one case `precision_codec`
/// admits a 16-bit RGB TIFF with extra samples.
pub(crate) fn tiff_spot_extras_accounted(bytes: &[u8]) -> bool {
    let run = crate::metadata::scan(bytes).metadata.iptc;
    tiff_spot_records(bytes, run.as_deref())
        .is_some_and(|(layout, records)| layout.saved() > 0 && records.kept.len() == layout.saved())
}

/// The resources, ExtraSamples and planes of `inks` (module doc, WRITE).
fn spot_planes(
    inks: &[(&str, [f64; 3], u8)],
    planes: Vec<Vec<u16>>,
    transparency: bool,
) -> TiffSpotPlanes {
    let extra_samples = transparency
        .then_some(TRANSPARENCY_EXTRA_SAMPLE)
        .into_iter()
        .chain(std::iter::repeat_n(SPOT_EXTRA_SAMPLE, inks.len()))
        .collect();
    TiffSpotPlanes {
        // LEAD L1: the transparency's entry leads every list.
        resources: encode_resources(inks, transparency),
        extra_samples,
        planes,
    }
}

/// `lines` appended to `warnings`, within the import report's cap.
fn joined(mut warnings: Vec<String>, lines: Vec<String>) -> Vec<String> {
    for line in lines {
        if warnings.len() >= MAX_IMPORT_WARNINGS {
            break;
        }
        warnings.push(line);
    }
    warnings
}

/// True when some pixel of a typed composite is not opaque: the
/// transparency a kept-spot TIFF writes as its first extra sample.
pub(crate) fn transparent16(image: &PrecisionImage) -> bool {
    image
        .runs_rgba16()
        .is_some_and(|mut runs| runs.any(|run| run.iter().any(|p| p[3] < u16::MAX)))
}

/// [`transparent16`] for an 8-bit composite.
pub(crate) fn transparent8(image: &image::RgbaImage) -> bool {
    image.pixels().any(|p| p[3] < 255)
}

/// A layered TIFF container's saved channels' reading (module doc, LAYERED):
/// its own file's extra samples described by the resources of its tag 34377;
/// nothing kept and no saved channel counted for a PSD or PSB, or a TIFF
/// with no described extra sample.
pub(crate) fn layered_spot_records(container: &Container<'_>) -> SpotRecords {
    container
        .tiff_file
        .and_then(|file| tiff_spot_records(file, Some(container.resources)))
        .map_or(
            SpotRecords {
                kept: Vec::new(),
                alpha: 0,
                losses: Vec::new(),
            },
            |(_, records)| records,
        )
}

/// The spot channels a layered TIFF writes (module doc, LAYERED): their
/// resources, the transparency's entry leading every list, and their
/// coverage planes, borrowed in place. Both empty without a spot channel.
pub(crate) struct LayeredTiffSpots<'a> {
    pub resources: Vec<u8>,
    pub planes: Vec<SpotCoverage<'a>>,
}

impl LayeredTiffSpots<'_> {
    fn of<'a>(
        inks: &[(&str, [f64; 3], u8)],
        planes: Vec<SpotCoverage<'a>>,
    ) -> LayeredTiffSpots<'a> {
        LayeredTiffSpots {
            // A layered TIFF always carries its transparency (L1); no
            // spot channel writes no resource.
            resources: encode_resources(inks, true),
            planes,
        }
    }
}

impl DrDocument {
    /// This document with the TIFF's kept spot channels appended (module
    /// doc); unchanged when there are none, the channel caps would break or
    /// a plane cannot be read (the report names it).
    pub(crate) fn with_tiff_spots(self, bytes: &[u8]) -> Result<Self, String> {
        let found = tiff_spot_records(bytes, self.metadata.iptc.as_deref());
        self.adopt_tiff_spots(bytes, found)
    }

    /// [`Self::with_tiff_spots`] for a layered TIFF container (module doc,
    /// LAYERED): the file's extra samples, described by its tag 34377.
    pub(crate) fn with_layered_tiff_spots(self, container: &Container<'_>) -> Result<Self, String> {
        let Some(file) = container.tiff_file else {
            return Ok(self);
        };
        let found = tiff_spot_records(file, Some(container.resources));
        self.adopt_tiff_spots(file, found)
    }

    /// The flat and the layered open's one body: `found`'s kept spot
    /// channels read from `bytes` and appended.
    fn adopt_tiff_spots(
        mut self,
        bytes: &[u8],
        found: Option<(TiffSpotLayout, SpotRecords)>,
    ) -> Result<Self, String> {
        let Some((layout, records)) = found else {
            return Ok(self);
        };
        if records.kept.is_empty()
            || (layout.width, layout.height) != (self.width, self.height)
            || !self.channels_fit(records.kept.len())
        {
            return Ok(self);
        }
        let saved: Vec<usize> = records.kept.iter().map(|(index, _, _)| *index).collect();
        let Ok(planes) = read_saved_planes(bytes, &layout, &saved) else {
            return Ok(self);
        };
        let narrow = Depth { eight: true };
        for ((_, name, ink), plane) in records.kept.into_iter().zip(planes) {
            let data: Vec<u8> = if layout.bytes == 2 {
                let mut out = Vec::with_capacity(plane.len());
                for v in plane {
                    narrow.push(&mut out, v);
                }
                out
            } else {
                plane.into_iter().map(|v| v as u8).collect()
            };
            let data = GrayImage::from_raw(self.width, self.height, data)
                .ok_or("a spot channel plane does not match the canvas")?;
            self.channels.push(Channel {
                spot: Some(ink),
                ..Channel::new(native_name(&name), Arc::new(data))
            });
        }
        Ok(self)
    }

    /// Every spot channel as an 8-bit RGB TIFF writes it (module doc),
    /// with `transparency` when the file keeps one.
    pub(crate) fn tiff_spot_planes(&self, transparency: bool) -> TiffSpotPlanes {
        let spots: Vec<(&Channel, SpotInk)> = self
            .channels
            .iter()
            .filter_map(|c| c.spot.map(|ink| (c, ink)))
            .collect();
        let inks: Vec<(&str, [f64; 3], u8)> = spots
            .iter()
            .map(|(c, ink)| (c.name.as_str(), ink.lab(), ink.solidity()))
            .collect();
        let planes = spots
            .iter()
            .map(|(c, _)| {
                c.data
                    .as_raw()
                    .iter()
                    .map(|&v| tiff_stored(u16::from(v), 255))
                    .collect()
            })
            .collect();
        spot_planes(&inks, planes, transparency)
    }

    /// The first [`spots_capacity`] spot channels as this document's layered
    /// TIFF writes them (module doc, LAYERED; four composite samples).
    pub(crate) fn layered_tiff_spots(&self) -> LayeredTiffSpots<'_> {
        let spots: Vec<(&Channel, SpotInk)> = self
            .channels
            .iter()
            .filter_map(|c| c.spot.map(|ink| (c, ink)))
            .take(spots_capacity(4))
            .collect();
        let inks: Vec<(&str, [f64; 3], u8)> = spots
            .iter()
            .map(|(c, ink)| (c.name.as_str(), ink.lab(), ink.solidity()))
            .collect();
        let planes = spots
            .iter()
            .map(|(c, _)| SpotCoverage::Eight(c.data.as_raw()))
            .collect();
        LayeredTiffSpots::of(&inks, planes)
    }
}

impl PrecisionDocument {
    /// This document with the TIFF's kept spot channels appended (module
    /// doc), admitted against `limit` for the decoded samples and the kept
    /// planes; the observation and every loss line join the import warnings.
    pub(crate) fn with_tiff_spots(
        self,
        bytes: &[u8],
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let Some((layout, mut records)) = tiff_spot_records(bytes, self.metadata().iptc.as_deref())
        else {
            return Ok(self);
        };
        let (doc, added) = self.adopt_tiff_spots(bytes, &layout, &mut records, limit, other)?;
        let mut lines: Vec<String> = records.observation().into_iter().collect();
        lines.extend(records.losses);
        lines.extend(added);
        doc.with_spot_warnings(lines)
    }

    /// [`Self::with_tiff_spots`] for a layered TIFF container (module doc,
    /// LAYERED). Only the lines the open itself finds (the document's channel
    /// limit, an unreadable plane) join the import warnings: the observation
    /// and the file's own losses are the compatibility report's, as for a
    /// PSD.
    pub(crate) fn with_layered_tiff_spots(
        self,
        container: &Container<'_>,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let Some(file) = container.tiff_file else {
            return Ok(self);
        };
        let Some((layout, mut records)) = tiff_spot_records(file, Some(container.resources)) else {
            return Ok(self);
        };
        let (doc, added) = self.adopt_tiff_spots(file, &layout, &mut records, limit, other)?;
        doc.with_spot_warnings(added)
    }

    /// `lines` joined to the import warnings, within their cap.
    fn with_spot_warnings(self, lines: Vec<String>) -> Result<Self, String> {
        if lines.is_empty() {
            return Ok(self);
        }
        let warnings = joined(self.import_warnings.as_ref().clone(), lines);
        self.with_import_warnings(warnings)
    }

    /// The flat and the layered open's one body: `records`' kept spot
    /// channels read from `bytes`, admitted and appended, and the loss lines
    /// it finds itself; `records.kept` is cleared when none is retained.
    fn adopt_tiff_spots(
        mut self,
        bytes: &[u8],
        layout: &TiffSpotLayout,
        records: &mut SpotRecords,
        limit: u64,
        other: u64,
    ) -> Result<(Self, Vec<String>), String> {
        let mut added = Vec::new();
        let (w, h) = self.dimensions();
        let n = records.kept.len();
        if n > 0 && !channels_fit_at(w, h, self.channels.len() + n) {
            added.push(channel_limit_loss(n));
            records.kept.clear();
        }
        if !records.kept.is_empty() {
            let read = if (layout.width, layout.height) == (w, h) {
                Ok(())
            } else {
                Err("the TIFF's dimensions disagree".to_string())
            };
            let planes_bytes = (n as u64)
                .saturating_mul(u64::from(w) * u64::from(h))
                .saturating_mul(2);
            admit(
                self.retained_bytes(),
                layout.read_bytes().saturating_add(planes_bytes),
                other,
                limit,
            )?;
            let saved: Vec<usize> = records.kept.iter().map(|(index, _, _)| *index).collect();
            match read.and_then(|()| read_saved_planes(bytes, layout, &saved)) {
                Ok(planes) => {
                    let channels = Arc::make_mut(&mut self.channels);
                    for ((_, name, ink), plane) in records.kept.iter().zip(planes) {
                        let samples: Vec<u16> = if layout.bytes == 1 {
                            plane.into_iter().map(|v| v * 257).collect()
                        } else {
                            plane
                        };
                        let mut channel = PrecisionChannel::new(
                            native_name(name),
                            PrecisionPlane::adopt(w, h, samples)?,
                        )?;
                        channel.metadata.spot = Some(*ink);
                        channels.push(channel);
                    }
                }
                Err(e) => {
                    added.push(format!(
                        "Spot channel planes in this file are not readable ({e}); no spot channel is retained."
                    ));
                    records.kept.clear();
                }
            }
        }
        Ok((self, added))
    }

    /// Every spot channel as this document's TIFF writes it (module doc),
    /// at 8 bits when `eight`, with `transparency` when the file keeps one.
    pub(crate) fn tiff_spot_planes(&self, eight: bool, transparency: bool) -> TiffSpotPlanes {
        let spots: Vec<(&PrecisionChannel, SpotInk)> = self
            .channels
            .iter()
            .filter_map(|c| c.metadata.spot.map(|ink| (c, ink)))
            .collect();
        let inks: Vec<(&str, [f64; 3], u8)> = spots
            .iter()
            .map(|(c, ink)| (c.name(), ink.lab(), ink.solidity()))
            .collect();
        let depth = Depth { eight };
        let planes = spots
            .iter()
            .map(|(c, _)| {
                let samples = c.plane().samples();
                if eight {
                    let mut bytes = Vec::with_capacity(samples.len());
                    for &v in samples {
                        depth.push(&mut bytes, tiff_stored(v, 65535));
                    }
                    bytes.into_iter().map(u16::from).collect()
                } else {
                    samples.iter().map(|&v| tiff_stored(v, 65535)).collect()
                }
            })
            .collect();
        spot_planes(&inks, planes, transparency)
    }

    /// The first [`spots_capacity`] spot channels as this document's layered
    /// TIFF writes them (module doc, LAYERED), its 16-bit planes in place.
    pub(crate) fn layered_tiff_spots(&self) -> LayeredTiffSpots<'_> {
        let spots: Vec<(&PrecisionChannel, SpotInk)> = self
            .channels
            .iter()
            .filter_map(|c| c.metadata.spot.map(|ink| (c, ink)))
            .take(spots_capacity(self.psd_composite_planes()))
            .collect();
        let inks: Vec<(&str, [f64; 3], u8)> = spots
            .iter()
            .map(|(c, ink)| (c.name(), ink.lab(), ink.solidity()))
            .collect();
        let planes = spots
            .iter()
            .map(|(c, _)| SpotCoverage::Sixteen(c.plane().samples()))
            .collect();
        LayeredTiffSpots::of(&inks, planes)
    }

    /// The working memory a TIFF that keeps the spot channels adds to its
    /// save or export: the stored planes (u16 a sample), the file's spot
    /// samples at `sample_bytes`, the interleaved strip and its copies (at
    /// most three [`SPOT_STRIP_BYTES`], fewer for a canvas whose
    /// `colour_samples`, transparency and spots fit in less), and 512 bytes
    /// of resources a spot plus two bytes a UTF-16 unit of their names.
    pub(crate) fn tiff_spot_scratch_bytes(&self, sample_bytes: u64, colour_samples: u64) -> u64 {
        let (w, h) = self.dimensions();
        let pixels = u64::from(w) * u64::from(h);
        let spots: Vec<&PrecisionChannel> = self
            .channels
            .iter()
            .filter(|c| c.metadata.spot.is_some())
            .collect();
        let count = spots.len() as u64;
        if count == 0 {
            return 0;
        }
        let units: u64 = spots
            .iter()
            .map(|c| c.name().encode_utf16().count() as u64)
            .sum();
        let strip = pixels
            .saturating_mul(colour_samples + 1 + count)
            .saturating_mul(sample_bytes)
            .min(SPOT_STRIP_BYTES)
            .saturating_mul(3);
        count
            .saturating_mul(pixels)
            .saturating_mul(2 + sample_bytes)
            .saturating_add(strip)
            .saturating_add(count.saturating_mul(512))
            .saturating_add(units.saturating_mul(2))
    }

    /// Why a typed export `request` that keeps spot channels cannot be
    /// written: a CMYK document ([`CMYK_KEEP_REFUSAL`]; its TIFF save
    /// keeps them), or spot channels the `FlatTiff` rule does not carry
    /// ([`TIFF_REFUSAL`]). `None` for a request that does not keep them.
    pub(crate) fn tiff_keep_refusal(&self, request: &ExportRequest) -> Option<String> {
        if !request.keep_spots {
            return None;
        }
        if self.is_cmyk() {
            return Some(CMYK_KEEP_REFUSAL.into());
        }
        let sample_bytes = u64::from(request.bit_depth / 8);
        (self.has_spot_channel()
            && !spots_written_for(
                self.is_lab(),
                self.is_gray(),
                sample_bytes,
                SpotFile::FlatTiff,
            ))
        .then(|| TIFF_REFUSAL.into())
    }
}
