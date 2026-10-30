//! SPOT CHANNELS (#547, `designs/spot-channels.md`): a spot channel is
//! an existing channel (the byte `doc_channel::Channel`, the typed
//! `precision_mask::PrecisionChannel`) whose `spot` holds an ink. Everything
//! else is the alpha channel's: the canvas-sized plane, the count cap and
//! pixel budget, the geometry ops, duplicate, delete and load-as-selection.
//! The plane is the ink's COVERAGE at the document's channel depth (255 in a
//! byte document, 65535 in every typed one, means 100 % ink).
//!
//! The ink is stored as D50 CIE Lab in f64, the way Photoshop's descriptor
//! stores a spot colour and the way Duotone inks are already held here
//! (`duotone_ink`, whose range rule this module reuses), plus Solidity as
//! an integer percent, 0 to 100. Spot channels are allowed in RGB, CMYK,
//! Grayscale, Lab and Duotone and not in Bitmap or Indexed Color; the typed
//! add refuses those two by name, and so does a conversion INTO either
//! while a spot channel exists (`indexed_convert`, `indexed_document`,
//! `bitmap_convert`). Every other mode and depth conversion carries the ink
//! with its channel.
//!
//! How a spot channel shows on the composite is `spot_overprint`'s (the
//! byte view's adapter `spot_view`, the typed exports' `spot_typed`, CMYK's
//! ink on paper `spot_cmyk`); Merge Spot Channel and the alpha ↔ spot kind
//! change are `spot_merge`'s; how it persists is `spot_channel_native`'s.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::DrDocument;
use crate::doc_channel::Channel;
use crate::drdc::MAX_DRDC_NAME_LEN;
use crate::duotone_ink::{lab_in_range, lab_sentence};
use crate::job_control::DrJobControl;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};
use crate::precision_mask_edit::MaskEdit;

/// A spot channel's ink: D50 CIE Lab and Solidity in percent (module doc).
/// Built only through [`SpotInk::new`], so every value in a document is in
/// range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpotInk {
    lab: [f64; 3],
    solidity: u8,
}

/// The typed add on an Indexed Color document.
pub const INDEXED_REFUSAL: &str = "Spot channels are not available in Indexed Color mode.";
/// The typed add on a Bitmap document.
pub const BITMAP_REFUSAL: &str = "Spot channels are not available in Bitmap mode.";
/// A conversion to Indexed Color of a document holding a spot channel.
pub const TO_INDEXED_REFUSAL: &str =
    "Indexed Color has no spot channels; delete the spot channels first.";
/// A conversion to Bitmap of a document holding a spot channel.
pub const TO_BITMAP_REFUSAL: &str = "Bitmap has no spot channels; delete the spot channels first.";
/// A spot edit addressed to an alpha channel.
pub const NOT_SPOT_REFUSAL: &str = "the channel is not a spot channel";
/// A save of a document holding a spot channel to a container the writer
/// cannot carry it in: a Lab or 32-bit PSD/PSB (phase 1b writes the rest,
/// `psd_export_preflight::spots_written_for`).
pub const PSD_REFUSAL: &str = "Spot channels are not yet saved to a Lab or 32-bit PSD or PSB; save as .doctored, or delete the spot channels first.";
/// A TIFF save, flat or layered, of a document holding a spot channel that
/// the writer cannot carry (1c-2a and 1c-2b, `tiff_spot`:
/// `spots_written_for`'s TIFF rule): a Grayscale, Duotone, Lab or 32-bit
/// document.
pub const TIFF_REFUSAL: &str = "Spot channels are saved to a TIFF only from an RGB or CMYK document at 8 or 16 bits; save as .doctored, or delete the spot channels first.";
/// The flat save's loss line for a document holding a spot channel: the
/// host composites the overprint into the flattened file.
pub const FLAT_LOSS: &str = "spot channels (composited into the image as their overprint)";
/// Merge Spot Channel on a Duotone document (`spot_merge`): its one stored
/// channel is the inks' tone, so there is no colour channel to bake into.
pub const DUOTONE_MERGE_REFUSAL: &str = "Merge Spot Channel is not available in Duotone mode, whose one channel is its inks' tone; convert to Grayscale, RGB or CMYK Color first.";

impl SpotInk {
    /// An ink from D50 Lab (L 0..=100, a and b -128..=127, finite: the
    /// Duotone ink rule) and a solidity percent of at most 100.
    pub fn new(lab: [f64; 3], solidity: u8) -> Result<Self, String> {
        if !lab_in_range(lab) {
            return Err(lab_sentence("the spot ink colour"));
        }
        if solidity > 100 {
            return Err("solidity must be from 0 to 100 %".into());
        }
        Ok(Self { lab, solidity })
    }

    /// The ink colour, D50 CIE Lab.
    pub fn lab(&self) -> [f64; 3] {
        self.lab
    }

    /// Solidity in percent, 0..=100.
    pub fn solidity(&self) -> u8 {
        self.solidity
    }
}

impl DrDocument {
    /// Appends a spot channel named `name` with `ink`; `coverage` is exactly
    /// width × height bytes, row 0 top (255 = full ink). `None` on a zero
    /// canvas, a wrong length, or a list that would break either channel cap
    /// (`channels_fit`), as `add_channel` refuses; there is no resampling.
    pub fn add_spot_channel(&self, name: &str, ink: SpotInk, coverage: &[u8]) -> Option<Self> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let expected = (self.width as usize).checked_mul(self.height as usize)?;
        if coverage.len() != expected || !self.channels_fit(1) {
            return None;
        }
        let data = GrayImage::from_raw(self.width, self.height, coverage.to_vec())?;
        let mut doc = self.clone();
        doc.channels.push(Channel {
            spot: Some(ink),
            ..Channel::new(name, Arc::new(data))
        });
        Some(doc)
    }

    /// Sets spot channel `i`'s name (`None` keeps it) and ink in one step,
    /// Spot Channel Options' one undo step. `None` for an out-of-range index,
    /// an alpha channel, or when nothing would change.
    pub fn set_spot_channel(&self, i: usize, name: Option<&str>, ink: SpotInk) -> Option<Self> {
        let channel = self.channels.get(i)?;
        let current = channel.spot?;
        let name = name.unwrap_or(&channel.name);
        if current == ink && channel.name == name {
            return None;
        }
        let mut doc = self.clone();
        let channel = &mut doc.channels[i];
        channel.name = name.to_string();
        channel.spot = Some(ink);
        Some(doc)
    }
}

impl PrecisionDocument {
    /// True when some channel is a spot channel.
    pub(crate) fn has_spot_channel(&self) -> bool {
        self.channels.iter().any(|c| c.metadata.spot.is_some())
    }

    /// Appends a spot channel. An Indexed document refuses with
    /// [`INDEXED_REFUSAL`] and a Bitmap one with [`BITMAP_REFUSAL`] before
    /// anything else; otherwise this is the alpha channel's append
    /// (`MaskEdit::SetChannel` with no index: its name limit, channel caps,
    /// plan and admission; `coverage` canvas-sized u16, 65535 = full ink)
    /// with the ink stamped on the appended channel.
    pub fn add_spot_channel(
        &self,
        name: &str,
        ink: SpotInk,
        coverage: &[u16],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        if self.is_indexed() {
            return Err(INDEXED_REFUSAL.into());
        }
        if self.is_bitmap() {
            return Err(BITMAP_REFUSAL.into());
        }
        let edit = MaskEdit::SetChannel {
            index: None,
            name: name.to_string(),
        };
        let mut doc = self
            .mask_edit(&edit, coverage, limit, other, control)?
            .ok_or("appending the spot channel changed nothing")?;
        // The appended channel is the fresh document's own; `make_mut`
        // copies the list only if something else still shares it.
        Arc::make_mut(&mut doc.channels)
            .last_mut()
            .ok_or("appending the spot channel changed nothing")?
            .metadata
            .spot = Some(ink);
        Ok(doc)
    }

    /// Sets spot channel `index`'s name (`None` keeps it) and ink. `Err` for
    /// an index out of range, an alpha channel ([`NOT_SPOT_REFUSAL`]) or a
    /// name past the native limit; `Ok(None)` when nothing changes. Admits
    /// `ChannelProperties`' plan, four bookkeeping units; no plane is copied.
    pub fn set_spot_channel(
        &self,
        index: usize,
        name: Option<&str>,
        ink: SpotInk,
        limit: u64,
        other: u64,
    ) -> Result<Option<Self>, String> {
        let channel = self
            .channels
            .get(index)
            .ok_or("typed channel index is out of range")?;
        let current = channel.metadata.spot.ok_or(NOT_SPOT_REFUSAL)?;
        if name.is_some_and(|s| s.len() > MAX_DRDC_NAME_LEN as usize) {
            return Err("typed channel name exceeds native limit".into());
        }
        let name = name.unwrap_or(channel.name());
        if current == ink && channel.name() == name {
            return Ok(None);
        }
        admit(self.retained_bytes(), 4 * BOOKKEEPING_BYTES, other, limit)?;
        let mut channels = self.channels.as_ref().clone();
        let channel = &mut channels[index];
        channel.name = Arc::new(name.to_string());
        channel.metadata.spot = Some(ink);
        Ok(Some(self.rebuilt(
            self.width,
            self.height,
            self.layers.as_ref().clone(),
            channels,
        )?))
    }
}
