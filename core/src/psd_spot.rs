//! #1 spot channels 1b-1: PSD and PSB spot channels in and out, the
//! adapter over the `psd_spot_records` kernel (resources 1006/1045/1077/1053
//! and the polarity).
//!
//! READ ([`Container::spot_records`]). A file in RGB, CMYK, Grayscale or
//! Duotone at 8 or 16 bits, PSD or PSB ([`reads_spots`]), has saved
//! channels after its colour planes and merged alpha: the first is plane
//! `base + merged alpha` (RGB 3, CMYK 4, Gray 1), and every plane after it is
//! one. Those whose DisplayInfo kind is spot, with a Lab ink in range, are
//! KEPT; the rest stay the disclosed loss they were before (`alpha`, the
//! existing "additional saved channels" sentences). The spot planes' framing
//! is checked before any open uses them — the Raw length, or every kept
//! plane's RLE rows through `psd_framing::rle_rows` and `validate_packbits`,
//! as `Container::composite_planes` checks the planes it skips — so a plane
//! that cannot be read is a loss line with nothing kept, the layered open
//! still succeeds, and the report and every open agree. A ZIP composite's
//! spot planes are not read (disclosed). Every open site calls
//! [`DrDocument::with_psd_spots`] or [`PrecisionDocument::with_psd_spots`],
//! which decode only the kept planes, one at a time, coverage = `max −
//! stored`; an 8-bit plane in a typed document is ×257.
//!
//! WRITE (`psd_spot_planes`). The first [`spots_capacity`] spot channels in
//! channel order become PackBits rows at the file's depth (a typed 8-bit
//! file through `psd_export_cmyk::Depth`'s `round(v / 257)`, after the
//! polarity), and `psd_export_encode::write_psd` declares them after the
//! composite with their resources. Alpha channels are not written.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::DrDocument;
use crate::doc_channel::{channels_fit_at, Channel};
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::admit;
use crate::precision_mask::{PrecisionChannel, PrecisionPlane};
use crate::psd_container::Container;
use crate::psd_export_cmyk::Depth;
use crate::psd_export_encode::packbits_row;
use crate::psd_export_preflight::{spot_bytes, spots_capacity};
use crate::psd_framing::{rle_rows, unpack_packbits, validate_packbits, LayeredContainer};
use crate::psd_spot_records::{
    read_saved, spot_lab, stored, DISPLAY_INFO, KIND_SPOT, LAB_SPACE, NAMES, UNICODE_NAMES,
};
use crate::spot_channel::SpotInk;
use crate::tiff_spot::layered_spot_records;

/// What a file's saved channels give (module doc): the kept spot channels
/// as (saved index, name, ink), the count of saved channels NOT kept, and the
/// report's loss lines.
pub(crate) struct SpotRecords {
    pub kept: Vec<(usize, String, SpotInk)>,
    pub alpha: usize,
    pub losses: Vec<String>,
}

impl SpotRecords {
    /// The import observation naming every kept spot channel.
    pub(crate) fn observation(&self) -> Option<String> {
        if self.kept.is_empty() {
            return None;
        }
        let names: Vec<String> = self
            .kept
            .iter()
            .map(|(_, name, _)| format!("\u{201c}{name}\u{201d}"))
            .collect();
        Some(format!(
            "{} spot channels open as spot channels with their names, inks (D50 Lab) and solidity: {}.",
            self.kept.len(),
            names.join(", ")
        ))
    }
}

/// A file's spot channels as `write_psd` writes them: each (name, D50 Lab,
/// Solidity) in channel order, and their PackBits rows, plane-major.
#[derive(Default)]
pub(crate) struct SpotPlanes {
    pub inks: Vec<(String, [f64; 3], u8)>,
    pub rows: Vec<Vec<u8>>,
}

/// True for the files whose spot channels are read: colour mode 1, 3, 4 or
/// 8 at 8 or 16 bits, in a PSD or PSB.
pub(crate) fn reads_spots(container: &Container<'_>) -> bool {
    matches!(container.color, 1 | 3 | 4 | 8)
        && matches!(container.depth, 8 | 16)
        && container.kind != LayeredContainer::Tiff
}

fn base_planes(color: u16) -> usize {
    match color {
        3 => 3,
        4 => 4,
        _ => 1,
    }
}

/// `name` cut to the native name limit on a character boundary (a TIFF's
/// spot channels too, `tiff_spot`).
pub(crate) fn native_name(name: &str) -> &str {
    let mut end = name.len().min(MAX_DRDC_NAME_LEN as usize);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// The spot channels among a file's `count` saved channels, read from its
/// resources (`resource(id)`: 1006, 1045, 1077), as (saved index, name,
/// ink): every channel of kind spot with a Lab ink `SpotInk::new` holds.
/// `merged` lets the lists lead with the merged transparency's entry. A
/// resource mismatch, a non-Lab ink, an ink past the range and a count past
/// the channel caps of a `width` × `height` canvas are `out`'s loss lines;
/// empty when nothing is kept. The PSD container and the flat TIFF
/// (`tiff_spot`) share it, so both name a file's losses alike.
pub(crate) fn spot_candidates<'r>(
    resource: impl Fn(u16) -> Option<&'r [u8]>,
    count: usize,
    merged: bool,
    (width, height): (u32, u32),
    out: &mut SpotRecords,
) -> Vec<(usize, String, SpotInk)> {
    let Some(display) = resource(DISPLAY_INFO) else {
        return Vec::new();
    };
    let saved = match read_saved(
        resource(NAMES),
        resource(UNICODE_NAMES),
        Some(display),
        count,
        merged,
    ) {
        Ok(saved) => saved,
        Err(_) => {
            out.losses.push(format!("This file's saved-channel resources (1006, 1045, 1077) do not match its {count} saved channels; no spot channel is retained."));
            return Vec::new();
        }
    };
    let mut candidates = Vec::new();
    for (index, channel) in saved.into_iter().enumerate() {
        if channel.display.kind != KIND_SPOT {
            continue;
        }
        let name = channel.name;
        if channel.display.color_space != LAB_SPACE {
            out.losses.push(format!(
                "Spot channel \u{201c}{name}\u{201d} stores its ink in colour space {}, which Doctored does not read; it is not retained.",
                channel.display.color_space
            ));
            continue;
        }
        match spot_lab(&channel.display).and_then(|(lab, s)| SpotInk::new(lab, s).ok()) {
            Some(ink) => candidates.push((index, name, ink)),
            None => out.losses.push(format!(
                "Spot channel \u{201c}{name}\u{201d} stores an ink Doctored cannot hold; it is not retained."
            )),
        }
    }
    let n = candidates.len();
    if n > 0 && !channels_fit_at(width, height, n) {
        out.losses.push(channel_limit_loss(n));
        return Vec::new();
    }
    candidates
}

/// The loss line for `n` spot channels a file holds but Doctored's channel
/// limits cannot admit; shared by the PSD and TIFF spot readers.
pub(crate) fn channel_limit_loss(n: usize) -> String {
    format!("{n} spot channels exceed Doctored's channel limits; none is retained.")
}

/// The loss line for a file's spot channel planes that Doctored cannot
/// read, for `reason`; shared by the PSD and TIFF spot readers.
pub(crate) fn planes_unreadable_loss(reason: &str) -> String {
    format!("Spot channel planes in this file are not readable ({reason}); no spot channel is retained.")
}

fn word_at(bytes: &[u8]) -> Result<u16, String> {
    bytes
        .get(..2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| "truncated PSD composite".to_string())
}

impl Container<'_> {
    /// The first saved plane and the saved channels' count (module doc).
    fn saved_span(&self) -> (usize, usize) {
        let base = base_planes(self.color);
        let channels = usize::from(self.channels);
        let first = base + usize::from(self.merged_alpha && channels > base);
        (first, channels.saturating_sub(first))
    }

    /// The saved channels' reading (module doc); a layered TIFF's are its
    /// composite's extra samples (`tiff_spot::layered_spot_records`). No
    /// plane is decoded.
    pub(crate) fn spot_records(&self) -> SpotRecords {
        if self.tiff_file.is_some() {
            return layered_spot_records(self);
        }
        let (first, count) = self.saved_span();
        let mut out = SpotRecords {
            kept: Vec::new(),
            alpha: count,
            losses: Vec::new(),
        };
        if count == 0 || !reads_spots(self) {
            return out;
        }
        let merged = first > base_planes(self.color);
        let candidates = spot_candidates(
            |id| self.resource(id).ok().flatten(),
            count,
            merged,
            (self.width, self.height),
            &mut out,
        );
        if candidates.is_empty() {
            return out;
        }
        let n = candidates.len();
        match self.spot_framing(first, &candidates) {
            Ok(true) => {}
            Ok(false) => {
                out.losses.push(format!(
                    "This file's stored composite is ZIP-compressed; its {n} spot channels are not read."
                ));
                return out;
            }
            Err(e) => {
                out.losses.push(planes_unreadable_loss(&e));
                return out;
            }
        }
        out.alpha = count - n;
        out.kept = candidates;
        out
    }

    /// The kept planes' framing (module doc): `Ok(false)` for a ZIP
    /// composite, whose planes are not read.
    fn spot_framing(
        &self,
        first: usize,
        kept: &[(usize, String, SpotInk)],
    ) -> Result<bool, String> {
        let pixels = self.width as usize * self.height as usize;
        let sample_bytes = self.sample_bytes() as usize;
        let channels = usize::from(self.channels);
        let height = self.height as usize;
        match word_at(self.composite)? {
            0 => {
                let expected = pixels
                    .checked_mul(channels * sample_bytes)
                    .and_then(|v| v.checked_add(2))
                    .ok_or("PSD composite size overflow")?;
                if self.composite.len() != expected {
                    return Err("PSD raw composite length mismatch".into());
                }
                Ok(true)
            }
            1 => {
                let rows = rle_rows(&self.composite[2..], channels * height, self.kind.large())?;
                let row_bytes = self.width as usize * sample_bytes;
                for (saved, _, _) in kept {
                    let plane = first + saved;
                    for row in &rows[plane * height..(plane + 1) * height] {
                        validate_packbits(row, row_bytes)?;
                    }
                }
                Ok(true)
            }
            2 | 3 => Ok(false),
            _ => Err("PSD stored composite uses unsupported compression".into()),
        }
    }

    /// Saved channel `saved`'s plane as COVERAGE at the file's depth
    /// (`sample_bytes` 1, or 2 big-endian), the polarity undone.
    pub(crate) fn spot_plane(&self, saved: usize, sample_bytes: usize) -> Result<Vec<u8>, String> {
        let (first, count) = self.saved_span();
        if saved >= count {
            return Err("the saved channel is out of range".into());
        }
        let plane = first + saved;
        let pixels = self.width as usize * self.height as usize;
        let height = self.height as usize;
        let row_bytes = self.width as usize * sample_bytes;
        let mut out = match word_at(self.composite)? {
            0 => {
                let start = 2 + plane * pixels * sample_bytes;
                self.composite
                    .get(start..start + pixels * sample_bytes)
                    .ok_or("PSD raw composite length mismatch")?
                    .to_vec()
            }
            1 => {
                let rows = rle_rows(
                    &self.composite[2..],
                    usize::from(self.channels) * height,
                    self.kind.large(),
                )?;
                let mut out = Vec::with_capacity(pixels * sample_bytes);
                for row in &rows[plane * height..(plane + 1) * height] {
                    out.extend_from_slice(&unpack_packbits(row, row_bytes)?);
                }
                out
            }
            _ => return Err("PSD spot planes are read only from a Raw or RLE composite".into()),
        };
        if sample_bytes == 2 {
            for pair in out.chunks_exact_mut(2) {
                let v = stored(u16::from_be_bytes([pair[0], pair[1]]), 65535);
                pair.copy_from_slice(&v.to_be_bytes());
            }
        } else {
            for v in &mut out {
                *v = stored(u16::from(*v), 255) as u8;
            }
        }
        Ok(out)
    }

    /// The kept planes' retained bytes at `sample_bytes` a sample.
    pub(crate) fn spot_retained_bytes(&self, sample_bytes: u64) -> u64 {
        (self.spot_records().kept.len() as u64)
            .saturating_mul(u64::from(self.width) * u64::from(self.height))
            .saturating_mul(sample_bytes)
    }
}

/// The export inventory's spot counts, `(channels, name UTF-16 units)`, from
/// the names of a document's spot channels: the one fill the byte, 16-bit
/// and CMYK exports share.
pub(crate) fn spot_inventory<'a>(names: impl Iterator<Item = &'a str>) -> (u64, u64) {
    names.fold((0, 0), |(count, units), name| {
        (count + 1, units + name.encode_utf16().count() as u64)
    })
}

impl DrDocument {
    /// This document with the file's kept spot channels appended (module
    /// doc); unchanged when there are none or the channel caps would break.
    pub(crate) fn with_psd_spots(mut self, container: &Container<'_>) -> Result<Self, String> {
        if container.tiff_file.is_some() {
            return self.with_layered_tiff_spots(container);
        }
        let records = container.spot_records();
        if records.kept.is_empty() || container.depth != 8 || !self.channels_fit(records.kept.len())
        {
            return Ok(self);
        }
        for (saved, name, ink) in records.kept {
            let plane = container.spot_plane(saved, 1)?;
            let data = GrayImage::from_raw(self.width, self.height, plane)
                .ok_or("a spot channel plane does not match the canvas")?;
            self.channels.push(Channel {
                spot: Some(ink),
                ..Channel::new(native_name(&name), Arc::new(data))
            });
        }
        Ok(self)
    }

    /// The first [`spots_capacity`] spot channels as an 8-bit RGB file
    /// writes them (four composite planes).
    pub(crate) fn psd_spot_planes(&self) -> SpotPlanes {
        let mut out = SpotPlanes::default();
        let width = self.width as usize;
        let spots = self
            .channels
            .iter()
            .filter_map(|c| c.spot.map(|ink| (c, ink)));
        for (channel, ink) in spots.take(spots_capacity(4)) {
            out.inks
                .push((channel.name.clone(), ink.lab(), ink.solidity()));
            for row in channel.data.as_raw().chunks_exact(width) {
                let row: Vec<u8> = row
                    .iter()
                    .map(|&v| stored(u16::from(v), 255) as u8)
                    .collect();
                out.rows.push(packbits_row(&row));
            }
        }
        out
    }
}

impl PrecisionDocument {
    /// The composite planes this document's PSD writes: five for CMYK, one
    /// for Grayscale (and Duotone), four for RGB.
    pub(crate) fn psd_composite_planes(&self) -> usize {
        if self.is_cmyk() {
            5
        } else if self.is_gray() {
            1
        } else {
            4
        }
    }

    /// This document with the file's kept spot channels appended (module
    /// doc), admitted against `limit`; unchanged when there are none or the
    /// channel caps would break.
    pub(crate) fn with_psd_spots(
        mut self,
        container: &Container<'_>,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        if container.tiff_file.is_some() {
            return self.with_layered_tiff_spots(container, limit, other);
        }
        let records = container.spot_records();
        let (w, h) = self.dimensions();
        if records.kept.is_empty()
            || !channels_fit_at(w, h, self.channels.len() + records.kept.len())
        {
            return Ok(self);
        }
        admit(
            self.retained_bytes(),
            container.spot_retained_bytes(2),
            other,
            limit,
        )?;
        let sample_bytes = container.sample_bytes() as usize;
        let channels = Arc::make_mut(&mut self.channels);
        for (saved, name, ink) in records.kept {
            let plane = container.spot_plane(saved, sample_bytes)?;
            let samples: Vec<u16> = if sample_bytes == 2 {
                plane
                    .chunks_exact(2)
                    .map(|s| u16::from_be_bytes([s[0], s[1]]))
                    .collect()
            } else {
                plane.iter().map(|&v| u16::from(v) * 257).collect()
            };
            let mut channel =
                PrecisionChannel::new(native_name(&name), PrecisionPlane::adopt(w, h, samples)?)?;
            channel.metadata.spot = Some(ink);
            channels.push(channel);
        }
        Ok(self)
    }

    /// The first [`spots_capacity`] spot channels as this document's PSD
    /// writes them, at 8 bits when `eight`.
    pub(crate) fn psd_spot_planes(&self, eight: bool) -> SpotPlanes {
        let mut out = SpotPlanes::default();
        let depth = Depth { eight };
        let width = self.dimensions().0 as usize;
        let spots = self
            .channels
            .iter()
            .filter_map(|c| c.metadata.spot.map(|ink| (c, ink)));
        for (channel, ink) in spots.take(spots_capacity(self.psd_composite_planes())) {
            out.inks
                .push((channel.name().to_string(), ink.lab(), ink.solidity()));
            for samples in channel.plane().samples().chunks_exact(width) {
                let mut row = Vec::with_capacity(width * depth.bytes() as usize);
                for &v in samples {
                    depth.push(&mut row, stored(v, 65535));
                }
                out.rows.push(packbits_row(&row));
            }
        }
        out
    }

    /// The bytes [`psd_spot_planes`](Self::psd_spot_planes) and their
    /// resources can take in a file (`psd_export_preflight::spot_bytes`).
    pub(crate) fn psd_spot_worst_case(&self, eight: bool, large: bool) -> u64 {
        let (w, h) = self.dimensions();
        let spots = self.channels.iter().filter(|c| c.metadata.spot.is_some());
        let written: Vec<&PrecisionChannel> = spots
            .take(spots_capacity(self.psd_composite_planes()))
            .collect();
        let units: u64 = written
            .iter()
            .map(|c| c.name().encode_utf16().count() as u64)
            .sum();
        spot_bytes(
            written.len() as u64,
            units,
            u64::from(w) * if eight { 1 } else { 2 },
            u64::from(h),
            large,
        )
    }
}
