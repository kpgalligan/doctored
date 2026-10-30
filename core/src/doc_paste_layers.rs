//! Edit ▸ Paste of LAYERS:
//! a layer clipboard — a document built by `duplicate_to_new_document` — is
//! spliced into this document in ONE call at each depth,
//! `DrDocument::paste_layers` at 8 bits and `PrecisionDocument::paste_layers`
//! at 16 bits and 32-bit float. One call is one undo step.
//!
//! # The rule
//!
//! 1. **Slot.** `insert_slot(anchor, position)` resolves where the block
//!    lands (`layer_insert`'s rule, refusals by name), then the clip is
//!    refused by name when (typed) its float mode differs, or when its
//!    deepest record would sit past the depth's own nesting ceiling. A clip
//!    whose profile bytes differ from this document's is refused by name
//!    under `PasteProfile::Convert`; under `Preserve` (Don't Convert)
//!    it is RELABELLED instead, at 8 and 16 bits: first the Assign
//!    check of the clip against this document's profile refuses in Assign
//!    words (`DrDocument::assign_profile_refusal`, the typed
//!    `color_plan(_, false)`), then every record keeps its samples and is
//!    re-pointed at this document's profile (rule 3). At 8 and 16 bits
//!    conversion is the HOST's step, through the existing
//!    `promote_structure` / `narrow_structure` / `convert_to_profile`;
//!    nothing here converts. A
//!    32-bit FLOAT destination is the exception (measured
//!    against Photoshop): it never answers the profile or the depth
//!    refusal. A clip that is not float in its exact profile bytes is
//!    PROMOTED under the call's `PasteProfile`, record by record, by
//!    `layer_clipboard_float` (whose module doc is that rule), after the
//!    placement translation and before the splice; a record the float mode
//!    still cannot hold refuses the whole paste, naming the layer and the
//!    feature, before any admission. The call answers a `LayerPasteReport`
//!    of what the promotion changed (empty when nothing promoted).
//! 2. **Placement translation**, inside the same call and skipped when the
//!    offset is `(0, 0)`. The host computes it (`PastePlacement`'s centred
//!    formula over the pasted roots' union content box, or `(0, 0)` for
//!    Paste in Place). At 8 bits every clip entry goes through
//!    `doc_align::translate_entry` — the Move tool's rule: raster offsets
//!    move, linked masks move, a group moves only its linked mask, an
//!    unlinked mask stays. There is no lock gate: a paste is structure. At
//!    high depth the clip goes through `translate_records` with every record
//!    named, typed Move's rules and budget; its `None` (nothing moves) is the
//!    clip as it was.
//! 3. **Splice.** Every clip record, in order, lands at `slot.at`, its level
//!    raised by `slot.level`. Every record mints a fresh id; each distinct
//!    non-zero link id is re-numbered to the next unused id in the result
//!    (`next_link_id`'s rule), so a pasted link group never merges with one
//!    already here; every ROOT takes `slot.clipped || clipped`, so the block
//!    joins a clip run rather than re-basing it. Typed raster images are
//!    re-interpreted under this document's profile handle (`with_profile`,
//!    samples shared), adjustments are re-bound to it, and warp bindings at
//!    both depths are re-pointed to it (`effective_warp` compares profiles by
//!    pointer). Retained sources, descriptions, styles, locks, labels and open
//!    state ride verbatim. Under a relabel (rule 1) a retained source that is
//!    effective under the CLIP's profile stays bound, re-pointed without a
//!    re-render (byte `rendered_profile`, typed `rendered.with_profile`);
//!    one that is not is dropped, its pixels kept. An abstract Color Lookup look is re-baked through this
//!    document's profile (`doc_look` at 8 bits, the typed re-bind at
//!    16). The typed splice goes through `flatten` → `rebuild`
//!    → `rebuilt`, so this document's channels, comps, guides, evaluation,
//!    global light and tone map are kept.
//!
//! # CMYK (#1 phase 4c)
//!
//! Modes never mix in a layers paste: a CMYK document refuses a clip that is
//! not CMYK ([`CMYK_DESTINATION_REFUSAL`]) and an RGB document a CMYK clip
//! ([`RGB_DESTINATION_REFUSAL`]), and since Grayscale phase 4 a Gray clip
//! lands only in a Gray document and a Gray document takes only a Gray clip
//! ([`GRAY_MIX_REFUSAL`]), and since Lab phase 4c a Lab clip lands only in
//! a Lab document and a Lab document takes only a Lab clip
//! ([`LAB_MIX_REFUSAL`], asked first); the HOST converts the clip first,
//! through the mode conversions (`convert_to_cmyk` / `convert_cmyk_to_rgb` /
//! `convert_to_gray` / `convert_gray_to_rgb` / `convert_to_lab` /
//! `convert_lab_to_rgb`). Between
//! CMYK documents a clip of the same profile bytes splices as it is (ink kept
//! bit for bit); one of another profile runs every `CmykRaster` leaf through
//! `cmyk_convert::cmyk_to_cmyk_image` into this document's profile at its
//! declared depth under `Convert` (`cmyk_convert_tree::map_tree`, every
//! record's properties carried) and splices as it is under `Preserve`. A
//! placement offset on a CMYK clip shifts every `CmykRaster` offset and every
//! LINKED mask by arithmetic alone — `doc_align::translate_entry`'s rule;
//! never `translate_records`, whose CMYK arm is geometry's. The conversion is
//! charged as the clipboard's (`cmyk_payload_convert::to_cmyk_terms` over
//! every leaf's pixels) with the converted planes held, before any pixel
//! work.
//!
//! # Admission
//!
//! The byte paste is checked for native capacity by its FFI shim
//! (`fallible_doc_op`). The typed paste admits
//! [`PrecisionDocument::paste_layers_allocation`] over this document's
//! retained bytes before it builds anything: `clipboard_paste`'s insertion
//! scratch formula over both record lists — `4 · BOOKKEEPING + 2 · (every
//! name) + 4096 · (both record counts)` — because the pixels are the clip's
//! own allocations, shared, and what the splice builds is the rebuilt tree,
//! plus [`LAYER_PASTE_MARGIN_PER_PIXEL`] × the destination canvas, fitted
//! below. A promotion into float adds its own terms
//! (`layer_clipboard_float::promotion_terms`): the paste admits `conversion`
//! and then the insertion with `held` counted as other retained bytes and
//! `LAYER_PROMOTE_MARGIN_PER_PIXEL` × the promoted planes' pixels added to
//! the margin, both before any pixel work.
//! [`PrecisionDocument::paste_layers_plan`] is the exact threshold,
//! `max(retained + conversion, retained + held + insertion + margin)`, and
//! `paste_layers_allocation` the same without `retained`; with no promotion
//! both are the figures above.
//!
//! # Measurement record (batch 171, 2026-10-05)
//!
//! Fixture and method as `precision_clipboard_host`'s record: a Release app
//! on port 4997, 8 GiB, `MallocLargeCache=0`, one measured MCP call per
//! fresh instance, E = `ri_interval_max_phys_footprint` − the footprint
//! before the call. The case: a layers copy of the group, then ONE measured
//! `paste` (centred) into the same document — 6000×4000 16-bit `g16.rz` and
//! the 4500×3000 float fixture. A = [`PrecisionDocument::paste_layers_allocation`].
//!
//! - Without a margin: A = 4,231,372 at both depths (the pixels are shared;
//!   the tree is what the splice builds), against E = 1,248,871,288 /
//!   1,248,920,440 / 1,248,871,288 at 16 bits and 702,628,320 / 702,579,168
//!   / 702,677,472 at float. Nearly all of E is the publication's display
//!   preparation of the new projection (the 16-bit render plan's scratch is
//!   1,249,048,576), which happens inside the same paste call.
//! - Fitted: `⌈1.25 · max E⌉ − A` over the destination canvas is 64.87 B/px
//!   at 16 bits and 64.75 at float, so the margin is 65 B/px (64 gives
//!   1,540,231,372 < 1,561,150,550 at 16 bits).
//! - With the margin, re-measured on a rebuilt app: 16 bits E =
//!   1,248,871,288 / 1,248,838,520 / 1,248,838,520, A = 1,564,231,372, A /
//!   max E = 1.2525; float E = 702,579,168 / 702,579,168 / 702,562,784, A =
//!   881,731,372, 1.255. The `list_documents` twin E = 0 / 0 / 0. Both gates
//!   A ≥ ⌈1.25 × max E⌉ hold.
//!
//! # Measurement record (batch 172, 2026-10-05)
//!
//! A 6000×4000 (24 MP) float layers copy is held in process, so
//! the 24 MP float paste is measurable. Pasting the group back into batch
//! 171's 6000×4000 float fixture is refused by name before any build (the
//! result, about 2.69 GB, is past the 2 GiB native cap), so the copy of the
//! group lands in a one-layer 6000×4000 float document instead, in the same
//! process. E = 1,245,217,656 / 1,245,086,584 / 1,245,102,968, A =
//! 1,564,214,896, A / max E = 1.256; the `list_documents` twin E = 0 / 0 /
//! 0. The gate A ≥ ⌈1.25 × max E⌉ (1,556,522,070) holds, so the margin
//! stays 65 B/px.
//!
//! `DrDocument::paste_layers` (`dr_doc_paste_layers`, through `fallible_doc_op`) and
//! `PrecisionDocument::paste_layers` (`dr_precision_doc_paste_layers`) splice EVERY record of a
//! clip document (a `duplicate_to_new_document` result) at `insert_slot`'s slot in ONE call.
//! Refusals by name after the slot's: profile bytes under Convert (Preserve relabels after the
//! Assign check), (typed) float mode, nesting past the depth's
//! own ceiling (10 byte / 16 typed). A 32-bit FLOAT destination never answers the
//! profile or the depth refusal: the typed `paste_layers` takes a `PasteProfile` and returns
//! `(document, LayerPasteReport)`, and a clip that is not float in the document's exact profile
//! bytes is promoted per record by `layer_clipboard_float.rs` after the placement translation and
//! before the unchanged splice. A placement `(dx, dy)` is applied first inside the same call
//! through `doc_align::translate_entry` on every byte entry (no lock gate) or `translate_records`
//! over every typed record. Each record mints an id, each distinct link id takes the next unused
//! one (`next_link_id`, the typed one now `pub(crate)` in `precision_lock`), each ROOT takes
//! `slot.clipped || clipped`. The typed admission is `paste_layers_allocation` (`clipboard_paste`'s
//! insertion scratch over both record lists + `LAYER_PASTE_MARGIN_PER_PIXEL` 65 × canvas; its
//! module doc is the measurement record), and `paste_layers_plan` is the exact threshold,
//! `max(retained + conversion, retained + held + insertion + margin)`; both admissions run before
//! any pixel work. Tests: `tests/layer_clipboard.rs`.

use std::sync::Arc;

use crate::doc::{DrDocument, Layer};
use crate::doc_align::translate_entry;
use crate::doc_group::{validated, MAX_GROUP_DEPTH as BYTE_MAX_GROUP_DEPTH};
use crate::doc_group_query::next_link_id;
use crate::doc_identity::mint_layer_id;
use crate::doc_look::rebake_layer_meta;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::layer_clipboard_float::{
    promoted_layers, promotion_terms, refusal as float_refusal, LayerPasteReport, PromotionTerms,
    LAYER_PROMOTE_MARGIN_PER_PIXEL,
};
use crate::layer_insert::InsertPosition;
use crate::precision_clipboard::PasteProfile;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_lock::next_link_id as next_typed_link_id;
use crate::precision_render::MAX_GROUP_DEPTH as TYPED_MAX_GROUP_DEPTH;
use crate::precision_structure_ops::{flatten, rebuild, FlatRecord};

/// The fitted margin a typed layer paste charges per DESTINATION canvas
/// pixel on top of the insertion scratch. Why: the measured peak of a layer
/// paste (the module doc's record) is the new projection's display
/// preparation, which the splice's own terms do not see, and the house rule
/// asks the allocation figure to be at least 1.25× that peak; 65 is the
/// smallest integer that passes at 16 bits and float (64 does not).
pub(crate) const LAYER_PASTE_MARGIN_PER_PIXEL: u64 = 65;

const PROFILE_REFUSAL: &str = "pasted layers must be in the document's profile; convert them first";
const DEPTH_REFUSAL: &str = "pasted layers must be at the document's depth; convert them first";

/// A CMYK document's refusal of a clip that is not CMYK (module doc).
pub(crate) const CMYK_DESTINATION_REFUSAL: &str =
    "pasted layers must be CMYK to land in a CMYK document; convert them to CMYK first";

/// An RGB document's refusal of a CMYK clip (module doc).
pub(crate) const RGB_DESTINATION_REFUSAL: &str =
    "pasted CMYK layers must be converted to RGB before they land in an RGB document";

/// A layers paste mixing Lab Color and another mode (module doc).
const LAB_MIX_REFUSAL: &str = "Lab Color layers paste only into a Lab Color document, and a Lab Color document takes only Lab Color layers: convert the clip first";

/// A layers paste mixing Grayscale and another mode (module doc).
const GRAY_MIX_REFUSAL: &str = "Grayscale layers paste only into a Grayscale document, and a Grayscale document takes only Grayscale layers: convert the clip first";

/// `layers` with every CMYK raster offset and every LINKED mask moved by `d`
/// (module doc): `doc_align::translate_entry`'s rule over the typed tree.
fn cmyk_translated(
    layers: &[PrecisionDocumentLayer],
    d: (i32, i32),
) -> Vec<PrecisionDocumentLayer> {
    let shift = |at: (i32, i32)| (at.0.saturating_add(d.0), at.1.saturating_add(d.1));
    layers
        .iter()
        .map(|layer| {
            let mut out = layer.clone();
            match &mut out.content {
                Content::CmykRaster { offset, .. } => *offset = shift(*offset),
                Content::Group(children) | Content::PassThrough(children) => {
                    *children = Arc::new(cmyk_translated(children, d));
                }
                _ => {}
            }
            if let Some(mask) = out.mask.as_mut().filter(|m| m.properties.linked) {
                mask.properties.offset = shift(mask.properties.offset);
            }
            out
        })
        .collect()
}

/// The CMYK leaves' pixels of `layers`, at every depth.
fn cmyk_pixels(layers: &[PrecisionDocumentLayer]) -> u64 {
    layers
        .iter()
        .map(|layer| match (&layer.content, layer.children()) {
            (_, Some(children)) => cmyk_pixels(children),
            (Content::CmykRaster { image, .. }, None) => {
                let (w, h) = image.dimensions();
                u64::from(w) * u64::from(h)
            }
            _ => 0,
        })
        .sum()
}

/// A CMYK clip's conversion terms (module doc): the clipboard's leg over
/// every leaf's pixels, the converted planes held.
fn cmyk_conversion_terms(layers: &PrecisionDocument) -> Result<PromotionTerms, String> {
    let pixels = cmyk_pixels(layers.root_layers());
    Ok(PromotionTerms {
        conversion: crate::cmyk_payload_convert::to_cmyk_terms(pixels)?,
        held: pixels
            .checked_mul(crate::cmyk_image::CMYK_BYTES_PER_PIXEL)
            .ok_or("pasted layers size overflow")?,
        pixels: 0,
    })
}

/// The nesting refusal: `deepest` is the clip's deepest level, `level` the
/// slot's.
fn nesting(deepest: usize, level: usize, limit: usize) -> Result<(), String> {
    let n = deepest + level;
    if n > limit {
        return Err(format!(
            "pasting these layers here would nest them {n} levels deep; the limit is {limit}"
        ));
    }
    Ok(())
}

/// The relabel's byte record step (module doc, rule 3): a retained source
/// effective under the clip's profile `from` is re-pointed at `to` without a
/// re-render; one that is not is dropped, the record's pixels kept.
fn relabel_byte_source(layer: &mut Layer, from: &Arc<IccProfile>, to: &Arc<IccProfile>) {
    if layer.effective_source(from).is_none() {
        layer.retained_source = None;
    } else if let Some(source) = layer.retained_source.as_mut() {
        source.rendered_profile = to.clone();
    }
}

/// The relabel's typed record step, run before the record's raster is
/// re-interpreted under `to`: a retained source effective under the clip's
/// own labels has its fallback re-labelled (`with_profile`, samples shared);
/// one that is not is dropped.
fn relabel_typed_source(layer: &mut PrecisionDocumentLayer, to: &Arc<IccProfile>) {
    if layer.effective_source().is_none() {
        layer.retained_source = None;
    } else if let Some(source) = layer.retained_source.as_mut() {
        source.rendered = source.rendered.with_profile(to.clone());
    }
}

impl DrDocument {
    /// The layers of `layers` (every record, in order) spliced at the slot
    /// `position` names relative to `anchor`, translated by `offset` first —
    /// the module doc's rule. Refuses a slot by `insert_slot`'s sentence, a
    /// profile mismatch under `Convert`, the Assign check of a relabel under
    /// `Preserve`, and nesting past the byte ceiling by name.
    pub fn paste_layers(
        &self,
        layers: &DrDocument,
        anchor: usize,
        position: InsertPosition,
        offset: (i32, i32),
        profile: PasteProfile,
    ) -> Result<DrDocument, String> {
        let slot = self.insert_slot(anchor, position)?;
        if layers.layers.is_empty() {
            return Err("the layer clipboard holds no layers".into());
        }
        let relabel = layers.profile.bytes() != self.profile.bytes();
        if relabel {
            match profile {
                PasteProfile::Convert => return Err(PROFILE_REFUSAL.into()),
                PasteProfile::Preserve => {
                    if let Some(refusal) = layers.assign_profile_refusal(&self.profile) {
                        return Err(refusal);
                    }
                }
            }
        }
        let deepest = layers
            .layers
            .iter()
            .map(|l| usize::from(l.depth))
            .max()
            .unwrap_or(0);
        nesting(deepest, slot.level, usize::from(BYTE_MAX_GROUP_DEPTH))?;
        let mut clip = layers.layers.clone();
        if offset != (0, 0) {
            for i in 0..clip.len() {
                translate_entry(&mut clip, i, offset.0, offset.1);
            }
        }
        let old_links: Vec<u32> = clip.iter().map(|l| l.link).collect();
        let count = clip.len();
        for layer in &mut clip {
            let root = layer.depth == 0;
            layer.id = mint_layer_id();
            layer.depth = u16::try_from(usize::from(layer.depth) + slot.level)
                .map_err(|_| "pasted layer nesting overflow")?;
            if root {
                layer.clipped = slot.clipped || layer.clipped;
            }
            layer.link = 0;
            if let Some(warp) = layer.warp.as_mut() {
                warp.profile = self.profile.clone();
            }
            if relabel {
                relabel_byte_source(layer, &layers.profile, &self.profile);
                rebake_layer_meta(layer, &self.profile)?;
            }
        }
        let mut doc = self.clone();
        doc.layers.splice(slot.at..slot.at, clip);
        let block = slot.at..slot.at + count;
        let mut seen: Vec<u32> = Vec::new();
        for &link in &old_links {
            if link == 0 || seen.contains(&link) {
                continue;
            }
            seen.push(link);
            let id = next_link_id(&doc.layers);
            for (k, i) in block.clone().enumerate() {
                if old_links[k] == link {
                    doc.layers[i].link = id;
                }
            }
        }
        validated(doc).ok_or_else(|| "pasted layers would leave a malformed layer stack".into())
    }
}

impl PrecisionDocument {
    /// True when pasting `layers` here PROMOTES them: this document is
    /// 32-bit float and the clip is not float in its exact profile bytes.
    fn promotes(&self, layers: &PrecisionDocument) -> bool {
        self.is_float()
            && !(layers.is_float() && layers.profile().bytes() == self.profile().bytes())
    }

    /// True when pasting the CMYK clip `layers` here CONVERTS its ink: both
    /// are CMYK, their policies' profile bytes differ and `profile` is
    /// Convert (module doc).
    fn cmyk_converts(&self, layers: &PrecisionDocument, profile: PasteProfile) -> bool {
        match (self.cmyk_policy(), layers.cmyk_policy()) {
            (Some(own), Some(clip)) => {
                profile == PasteProfile::Convert && own.profile_bytes() != clip.profile_bytes()
            }
            _ => false,
        }
    }

    /// The promotion's terms (or a CMYK clip's conversion's), all 0 when
    /// nothing promotes or converts.
    fn paste_promotion(
        &self,
        layers: &PrecisionDocument,
        profile: PasteProfile,
    ) -> Result<PromotionTerms, String> {
        if self.cmyk_converts(layers, profile) {
            return cmyk_conversion_terms(layers);
        }
        if self.promotes(layers) {
            promotion_terms(layers, self.profile(), profile)
        } else {
            Ok(PromotionTerms {
                conversion: 0,
                held: 0,
                pixels: 0,
            })
        }
    }

    /// The typed twin of [`DrDocument::paste_layers`]: the same rule, plus
    /// the module doc's admission against `limit` with `other` retained
    /// elsewhere. A 16-bit document refuses a clip of another depth, and one
    /// of another profile under `Convert`; under `Preserve` it relabels that
    /// clip after the Assign check (`color_plan(_, false)`). A float document
    /// promotes it under `profile` instead (`layer_clipboard_float`) and
    /// answers what that changed.
    #[allow(clippy::too_many_arguments)]
    pub fn paste_layers(
        &self,
        layers: &PrecisionDocument,
        anchor: usize,
        position: InsertPosition,
        offset: (i32, i32),
        profile: PasteProfile,
        limit: u64,
        other: u64,
    ) -> Result<(PrecisionDocument, LayerPasteReport), String> {
        // #1 Indexed phase 2d: an Indexed document holds one layer.
        if self.is_indexed() {
            return Err(crate::indexed_document::refused_sentence("paste_layers"));
        }
        let slot = self.insert_slot(anchor, position)?;
        // #1 phase 4c: modes never mix here; the host converts first.
        if self.is_cmyk() && !layers.is_cmyk() {
            return Err(CMYK_DESTINATION_REFUSAL.into());
        }
        if !self.is_cmyk() && layers.is_cmyk() {
            return Err(RGB_DESTINATION_REFUSAL.into());
        }
        // #1 Lab phase 4c: nor do Lab Color and the other modes.
        if self.is_lab() != layers.is_lab() {
            return Err(LAB_MIX_REFUSAL.into());
        }
        // #1 phase 4: nor do Grayscale and the other modes.
        if self.is_gray() != layers.is_gray() {
            return Err(GRAY_MIX_REFUSAL.into());
        }
        let promotes = self.promotes(layers);
        let relabel = !promotes && layers.profile().bytes() != self.profile().bytes();
        if !promotes {
            if relabel && profile == PasteProfile::Convert {
                return Err(PROFILE_REFUSAL.into());
            }
            if layers.is_float() != self.is_float() {
                return Err(DEPTH_REFUSAL.into());
            }
            if relabel {
                layers.color_plan(self.profile(), false)?;
            }
        }
        let clip_flat = flatten(layers.root_layers());
        let deepest = clip_flat.iter().map(|r| r.depth).max().unwrap_or(0);
        nesting(deepest, slot.level, TYPED_MAX_GROUP_DEPTH)?;
        if promotes {
            if let Some(reason) = float_refusal(layers) {
                return Err(reason);
            }
        }
        let terms = self.paste_promotion(layers, profile)?;
        let retained = self.retained_bytes();
        // Both admissions precede any pixel work: the promotion's own, then
        // the insertion's with the promoted planes held.
        admit(retained, terms.conversion, other, limit)?;
        admit(
            retained,
            self.paste_insertion(layers, &terms)?,
            other
                .checked_add(terms.held)
                .ok_or("pasted layers size overflow")?,
            limit,
        )?;
        let translated;
        let source = if offset == (0, 0) {
            layers
        } else if layers.is_cmyk() {
            let (w, h) = layers.dimensions();
            translated = layers.rebuilt(
                w,
                h,
                cmyk_translated(layers.root_layers(), offset),
                Vec::new(),
            )?;
            &translated
        } else {
            let moves = vec![Some(offset); layers.layer_count()];
            let other = other
                .checked_add(retained)
                .ok_or("pasted layers size overflow")?;
            match layers.translate_records(&moves, limit, other, JobContext::NONE)? {
                Some(next) => {
                    translated = next;
                    &translated
                }
                None => layers,
            }
        };
        let (clip_flat, report) = if promotes {
            let (promoted, report) =
                promoted_layers(source, self.profile(), profile, JobContext::NONE)?;
            (flatten(&promoted), report)
        } else if self.cmyk_converts(layers, profile) {
            (
                flatten(&self.cmyk_converted_clip(source, JobContext::NONE)?),
                LayerPasteReport::default(),
            )
        } else {
            (flatten(source.root_layers()), LayerPasteReport::default())
        };
        let old_links: Vec<u32> = clip_flat.iter().map(|r| r.layer.link).collect();
        let mut block: Vec<FlatRecord> = Vec::with_capacity(clip_flat.len());
        for record in clip_flat {
            let mut layer = record.layer;
            if relabel {
                relabel_typed_source(&mut layer, self.profile());
            }
            layer.id = mint_layer_id();
            if record.depth == 0 {
                layer.clipped = slot.clipped || layer.clipped;
            }
            layer.link = 0;
            layer.content = match layer.content {
                Content::Raster { image, offset } => Content::Raster {
                    image: image.with_profile(self.profile().clone()),
                    offset,
                },
                Content::Adjustment(state) => Content::Adjustment(state.rebind(self.profile())?),
                other => other,
            };
            if let Some(warp) = layer.warp.as_mut() {
                warp.profile = self.profile().clone();
            }
            block.push(FlatRecord {
                layer,
                depth: record.depth + slot.level,
            });
        }
        let count = block.len();
        let mut flat = flatten(self.root_layers());
        if slot.at > flat.len() {
            return Err("layer insert anchor is out of range".into());
        }
        flat.splice(slot.at..slot.at, block);
        let mut seen: Vec<u32> = Vec::new();
        for &link in &old_links {
            if link == 0 || seen.contains(&link) {
                continue;
            }
            seen.push(link);
            let id = next_typed_link_id(&flat);
            for (k, i) in (slot.at..slot.at + count).enumerate() {
                if old_links[k] == link {
                    flat[i].layer.link = id;
                }
            }
        }
        let (width, height) = self.dimensions();
        let next = self.rebuilt(
            width,
            height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )?;
        Ok((next, report))
    }

    /// The CMYK clip `layers`' tree with every `CmykRaster` leaf's ink
    /// converted into this document's profile at its declared depth
    /// (`CmykToCmyk`, relative colorimetric with BPC), every record's
    /// properties carried and every layer style's inks through the same leg
    /// (module doc).
    fn cmyk_converted_clip(
        &self,
        layers: &PrecisionDocument,
        job: JobContext<'_>,
    ) -> Result<Vec<PrecisionDocumentLayer>, String> {
        let own = self.require_cmyk_policy("Paste")?;
        let clip = layers.require_cmyk_policy("Paste")?;
        let mut leg = crate::cmyk_color::CmykToCmyk::new(
            &clip.profile,
            &own.profile,
            crate::cmyk_color::ConversionOptions::default(),
        )?;
        let mut converted =
            crate::cmyk_convert_tree::map_tree(layers.root_layers(), &mut |layer| {
                let Content::CmykRaster { image, offset } = &layer.content else {
                    return Ok(layer.clone());
                };
                let image =
                    crate::cmyk_convert::cmyk_to_cmyk_image(image, &mut leg, own.depth(), job)?;
                PrecisionDocumentLayer::raster_cmyk(
                    &layer.name,
                    image,
                    *offset,
                    layer.opacity,
                    layer.visible,
                )
            })?;
        // A layer style's inks take the same leg, their hex this profile's
        // (#1, `cmyk_profile::convert_style_inks`).
        crate::cmyk_profile::convert_style_inks(&mut converted, &mut leg, own)?;
        Ok(converted)
    }

    /// The NEW memory a converting layers paste of `roots` (a
    /// `duplicate_to_new_document` clip of them, as if in another CMYK
    /// profile) admits: the descriptor's `cmyk_paste_layers_convert_scratch_bytes`
    /// (`cmyk_clipboard::cmyk_clipboard_plans`).
    pub(crate) fn cmyk_paste_layers_convert_plan(&self, roots: &[usize]) -> Result<u64, String> {
        let clip = self.duplicate_to_new_document(roots)?;
        let terms = cmyk_conversion_terms(&clip)?;
        let insertion = self
            .paste_insertion(&clip, &terms)?
            .checked_add(terms.held)
            .ok_or("pasted layers size overflow")?;
        Ok(terms.conversion.max(insertion))
    }

    /// The insertion's admission: clipboard_paste's insertion scratch formula
    /// (4·BOOKKEEPING + 2·(all names) + 4096·(both record counts)) +
    /// `LAYER_PASTE_MARGIN_PER_PIXEL`·canvas, plus
    /// `LAYER_PROMOTE_MARGIN_PER_PIXEL`·`terms.pixels` when a promotion runs.
    fn paste_insertion(
        &self,
        layers: &PrecisionDocument,
        terms: &PromotionTerms,
    ) -> Result<u64, String> {
        let names = |doc: &PrecisionDocument| -> u64 {
            (0..doc.layer_count())
                .filter_map(|i| doc.layer(i))
                .map(|layer| layer.name().len() as u64)
                .sum()
        };
        let records = (self.layer_count() + layers.layer_count()) as u64;
        let (w, h) = self.dimensions();
        let margin = LAYER_PASTE_MARGIN_PER_PIXEL
            .checked_mul(pixel_count(w, h)? as u64)
            .and_then(|n| n.checked_add(LAYER_PROMOTE_MARGIN_PER_PIXEL.checked_mul(terms.pixels)?))
            .ok_or("pasted layers size overflow")?;
        (4 * BOOKKEEPING_BYTES)
            .checked_add((names(self) + names(layers)) * 2)
            .and_then(|n| n.checked_add(records.checked_mul(4096)?))
            .and_then(|n| n.checked_add(margin))
            .ok_or_else(|| "pasted layers size overflow".into())
    }

    /// The paste's EXACT threshold with nothing retained elsewhere, this
    /// document's retained bytes included: `max(retained + conversion,
    /// retained + held + insertion + margin)` (module doc). With no
    /// promotion it is `retained + ` [`Self::paste_layers_allocation`].
    pub fn paste_layers_plan(
        &self,
        layers: &PrecisionDocument,
        profile: PasteProfile,
    ) -> Result<u64, String> {
        self.retained_bytes()
            .checked_add(self.paste_layers_allocation(layers, profile)?)
            .ok_or_else(|| "pasted layers size overflow".into())
    }

    /// The NEW memory the paste admits — the plan without this document's
    /// retained bytes: `max(conversion, held + insertion + margin)`; the
    /// insertion and its margin alone when nothing promotes.
    pub fn paste_layers_allocation(
        &self,
        layers: &PrecisionDocument,
        profile: PasteProfile,
    ) -> Result<u64, String> {
        let terms = self.paste_promotion(layers, profile)?;
        let insertion = self
            .paste_insertion(layers, &terms)?
            .checked_add(terms.held)
            .ok_or("pasted layers size overflow")?;
        Ok(terms.conversion.max(insertion))
    }
}
