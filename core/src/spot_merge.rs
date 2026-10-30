//! Merge Spot Channel and the alpha ↔ spot kind change (#547 1c-1,
//! `designs/spot-channels.md`), at byte and typed depth.
//!
//! # Merge Spot Channel
//!
//! The spot channel's ink is merged into the colour channels and the spot
//! channel is deleted, and a layered document is flattened first, with no
//! prompt, as Image ▸ Flatten Image does: the composite (hidden layers
//! discarded, the comp list cleared, every other channel and the document's
//! chrome kept) with the ONE ink baked in through the kernel the view uses
//! (`spot_view` at byte depth, `spot_typed` typed, `spot_cmyk`'s ink on
//! paper in CMYK), then the channel removed — one result, so one undo step.
//! A Duotone document refuses ([`DUOTONE_MERGE_REFUSAL`]): its one stored
//! channel is its inks' tone. Declared and unmeasured against Photoshop.
//!
//! The typed plan is the flatten's charge (`merge_scratch_bytes(&[])`) plus
//! the overprint's own term (`spot_overprint_scratch_bytes`), admitted before
//! anything is allocated. In CMYK it is `cmyk_flatten_scratch_bytes` alone:
//! the overprint fills the flatten's own on-paper plane in place.
//!
//! # Alpha ↔ spot
//!
//! A channel keeps its pixels when Channel Options changes its kind, and on
//! a spot channel black is 100 % ink. Doctored stores an alpha
//! channel as a selection (255 / 65535 = selected, shown white) and a spot
//! plane as ink coverage, so both directions invert the plane, `max − v`:
//! what showed black becomes full ink, and back. An alpha channel's
//! "Color Indicates" polarity is not read, and spot → alpha leaves its
//! rubylith colour and opacity as they were. The typed change is the
//! existing channel invert (`ScalarEdit`'s morphology invert, its own plan)
//! with the ink stamped or cleared after it. No `.rz` change: `SPOT` already
//! marks which channels are spot.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::doc::{DrDocument, Layer};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::admit;
use crate::precision_scalar_edit::ScalarEdit;
use crate::spot_channel::{
    SpotInk, BITMAP_REFUSAL, DUOTONE_MERGE_REFUSAL, INDEXED_REFUSAL, NOT_SPOT_REFUSAL,
};
use crate::spot_view::image_spot_overprint;
use crate::DrImage;

impl DrDocument {
    /// Merge Spot Channel on channel `i` (module doc): the flattened
    /// composite with that ink overprinted, as one "Background" layer, and
    /// the channel removed. `Ok(None)` for an index out of range or an alpha
    /// channel; `Err` when the flatten fails.
    pub fn merge_spot_channel(&self, i: usize) -> Result<Option<Self>, String> {
        let Some(ink) = self.channel_spot(i) else {
            return Ok(None);
        };
        let pixels = self.flattened()?;
        let printed = image_spot_overprint(
            &DrImage { pixels },
            self.channels[i].data.as_raw(),
            &ink,
            Some(&self.profile),
        )?;
        // `flattening`'s result, its one projection replaced by the printed
        // one: the same document with ONE new layer and no comps.
        let mut doc = self.clone();
        doc.layers = vec![Layer::new(printed.pixels, "Background")];
        doc.comps.clear();
        doc.channels.remove(i);
        Ok(Some(doc))
    }

    /// Channel `i` made a spot channel with `spot`, or an alpha channel with
    /// `None`, its plane inverted (module doc). `None` for an index out of
    /// range or a channel that is already that kind.
    pub fn set_channel_kind(&self, i: usize, spot: Option<SpotInk>) -> Option<Self> {
        let channel = self.channels.get(i)?;
        if channel.spot.is_some() == spot.is_some() {
            return None;
        }
        let mut doc = self.invert_channel(i)?;
        doc.channels[i].spot = spot;
        Some(doc)
    }
}

impl PrecisionDocument {
    /// Why Merge Spot Channel cannot run on channel `index`, or `None`.
    fn merge_spot_refusal(&self, index: usize) -> Option<String> {
        if self.is_duotone() {
            return Some(DUOTONE_MERGE_REFUSAL.into());
        }
        let Some(channel) = self.channels().get(index) else {
            return Some("typed channel index is out of range".into());
        };
        channel
            .metadata()
            .spot
            .is_none()
            .then(|| NOT_SPOT_REFUSAL.into())
    }

    /// The merge's working memory (module doc). In CMYK it is the flatten's
    /// charge alone: the overprint fills the flatten's own on-paper plane row
    /// by row (`spot_cmyk::cmyk_spot_on_paper`), so `cmyk_spot_scratch_bytes`
    /// would charge that plane twice.
    fn merge_spot_scratch_bytes(&self) -> Result<u64, String> {
        if self.is_cmyk() {
            return Ok(self.cmyk_flatten_scratch_bytes());
        }
        Ok(self
            .merge_scratch_bytes(&[])?
            .saturating_add(self.spot_overprint_scratch_bytes()?))
    }

    /// Merge Spot Channel's plan: `{"working_memory_plan_bytes": u64,
    /// "refusal": string|null}`; the refusal names a Duotone document, an
    /// index out of range or an alpha channel.
    pub fn merge_spot_channel_plan(&self, index: usize) -> Result<Value, String> {
        Ok(json!({
            "working_memory_plan_bytes": self.merge_spot_scratch_bytes()?,
            "refusal": self.merge_spot_refusal(index),
        }))
    }

    /// Merge Spot Channel on channel `index` (module doc): admitted against
    /// `limit` beside `other` before anything is allocated, then the
    /// render (CMYK: the composite on paper) with the one ink overprinted as
    /// one "Background" record, the channel removed and the comps cleared.
    /// `Err` for the plan's refusals, an admission refusal or a cancel.
    pub fn merge_spot_channel(
        &self,
        index: usize,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        if let Some(refusal) = self.merge_spot_refusal(index) {
            return Err(refusal);
        }
        let scratch = self.merge_spot_scratch_bytes()?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.reserve(scratch)?;
        let leaf = if self.is_cmyk() {
            self.require_cmyk_policy("Merge Spot Channel")?;
            let image = self.cmyk_spot_on_paper(&[index], job.range(0.0, 0.9))?;
            PrecisionDocumentLayer::raster_cmyk("Background", image, (0, 0), 1.0, true)?
        } else {
            let image = self.render_with_context(limit, other, job.range(0.0, 0.6))?;
            let printed = self.spot_overprinted(&image, &[index], job.range(0.6, 0.9))?;
            drop(image);
            self.merged_leaf("Background", printed, (0, 0))?
        };
        let (w, h) = self.dimensions();
        let mut channels = self.channels().to_vec();
        channels.remove(index);
        let mut next = self.rebuilt(w, h, vec![leaf], channels)?;
        // Flatten Image's rule (`flatten_image`): a flatten is not the same
        // document with changed records, so no comp survives it.
        next.comps.clear();
        job.report(1.0, 5)?;
        Ok(next)
    }

    /// Channel `index` made a spot channel with `spot`, or an alpha channel
    /// with `None` (module doc): the channel invert's plan admitted against
    /// `limit` beside `other`, then the invert and the ink. `Ok(None)` when
    /// the channel is already that kind; `Err` for an index out of range, an
    /// Indexed or Bitmap document asked for a spot channel (by sentence) and
    /// an admission refusal.
    pub fn set_channel_kind(
        &self,
        index: usize,
        spot: Option<SpotInk>,
        limit: u64,
        other: u64,
    ) -> Result<Option<Self>, String> {
        let channel = self
            .channels()
            .get(index)
            .ok_or("typed channel index is out of range")?;
        if spot.is_some() {
            if self.is_indexed() {
                return Err(INDEXED_REFUSAL.into());
            }
            if self.is_bitmap() {
                return Err(BITMAP_REFUSAL.into());
            }
        }
        if channel.metadata().spot.is_some() == spot.is_some() {
            return Ok(None);
        }
        let request = json!({
            "op": "scalar_edit",
            "target": "channel",
            "index": index,
            "operation": {"kind": "morphology", "operation": {"kind": "invert"}},
        });
        let edit = ScalarEdit::parse(&request.to_string())?;
        let plan = edit.plan(self)?;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        admit(self.retained_bytes(), scratch, other, limit)?;
        let mut next = edit
            .apply(self, &[], limit, other, JobContext::NONE)?
            .ok_or("inverting the channel changed nothing")?;
        Arc::make_mut(&mut next.channels)
            .get_mut(index)
            .ok_or("typed channel index is out of range")?
            .metadata
            .spot = spot;
        Ok(Some(next))
    }
}
