//! WHICH pixels a retouch gesture samples: the three sampling modes, the
//! ignore-adjustments control, and the [`Cut`] each pair derives.
//!
//! The modes are the retoucher's vocabulary, not a new compositor:
//!
//! * [`RetouchSampling::Current`] — the target layer's OWN pixels on a
//!   transparent canvas. Deliberately raw: opacity, blend mode, mask and
//!   visibility are compositing properties, so they play no part, exactly as
//!   [`DrDocument::layer_canvas_image`] states. This is the historic
//!   `sample_all_layers = false`.
//! * [`RetouchSampling::CurrentAndBelow`] — the document's own projection with
//!   every entry ABOVE the target contributing nothing.
//! * [`RetouchSampling::All`] — the document's own projection. This is the
//!   historic `sample_all_layers = true`.
//!
//! # "Above" is positional, and ancestors are not above
//!
//! The stack is a flattened forest, bottom first, in which a group's own
//! record CLOSES its subtree (`doc_group`'s module doc). So for a target at
//! `t`, every entry at an index below `t` is beneath it — a sibling lower in
//! the level, or something inside a group lower in the level — and every entry
//! above `t` is above it EXCEPT `t`'s own enclosing groups, whose records sit
//! past the subtree they close. Those ancestors keep contributing, because a
//! group's mask, opacity and blend describe how the level the target lives in
//! meets the picture; dropping them would sample a level that appears nowhere.
//!
//! # Ignoring adjustments can only mean the ones ABOVE
//!
//! An adjustment layer transforms the accumulator it is composited into
//! (`doc::composite_adjustment_into`), which holds everything already
//! composited BELOW it. So an adjustment below the target can never reach the
//! target's own pixels, and cloning a sample that carries it deposits exactly
//! what the eye already sees there. Only an adjustment ABOVE the target
//! applies to the pixels a retouch writes — twice, once in the sample and
//! again in the projection — so only those are cut.
//!
//! That makes `ignore_adjustments` a provable identity in `Current` (no
//! composite at all) and in `CurrentAndBelow` (every entry above the target is
//! already cut). It is the interesting control in `All`: take the texture of
//! the layers stacked above me, but not the grade they are being read
//! through.
//!
//! # The boundary, stated rather than implied
//!
//! Only an adjustment ENTRY is cut — an entry the compositor itself routes to
//! `composite_adjustment_into`. A layer STYLE, a Blend If, and a plain raster
//! layer in a tone blend mode (the grey Soft Light dodge-and-burn layer of
//! every retouching stack) change the picture in the same way and are NOT cut.
//! They are ordinary content, and a control that silently dropped them would
//! be sampling a document the user has never seen.
//!
//! The cut gates CONTRIBUTION (`composite_level_into`'s entry guards, `extent_of_level`,
//! `resolved_member`) and never `visible`, because `is_isolated` reads visibility: hiding a clipped
//! entry above the target would re-isolate its enclosing group and move pixels BELOW the target.
//! `Cut::NONE` is every other caller and is byte-identical. "Above" excludes the target's own
//! enclosing groups; only an adjustment ENTRY is cut, never a style, a Blend If or a tone blend
//! mode. Typed cuts are a `Cut` too: `precision_retouch_source.rs` and `precision_scoped_source.rs`
//! build the same postorder exclusion and render through `render_cut_with_context`, so a typed Pass
//! Through clip base keeps the document's isolation.

use crate::adjust::Adjustment;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group;
use crate::style_composite::Cut;
use image::RgbaImage;

/// Where a retouch gesture reads its source pixels from. The discriminants are
/// ABI: they cross the FFI as `uint32_t` and are named in the C header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetouchSampling {
    /// The target layer's own pixels, placed on a transparent canvas.
    Current = 0,
    /// The projection with everything above the target contributing nothing.
    CurrentAndBelow = 1,
    /// The whole projection.
    All = 2,
}

impl RetouchSampling {
    /// `None` for an unrecognized code — a domain refusal, never a silent
    /// fallback to a mode the caller did not ask for.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Current),
            1 => Some(Self::CurrentAndBelow),
            2 => Some(Self::All),
            _ => None,
        }
    }

    /// The wire name, which is also the MCP argument's spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::CurrentAndBelow => "current_and_below",
            Self::All => "all",
        }
    }

    /// True when the mode reads the target layer alone, so no composite —
    /// and therefore no cut, no budget and no group buffer — is involved.
    pub(crate) fn is_layer_only(self) -> bool {
        self == Self::Current
    }
}

/// The compositor's own adjustment test, which is `composite_layer_into`'s:
/// a GROUP's meta is the host's alone and is never interpreted, so a group
/// whose blob happens to parse as an adjustment is not one here either.
///
/// Deliberately NOT `Adjustment::in_profile`: a profile-incompatible
/// adjustment is refused for the whole render by
/// `adjust_calibrated::validate_context` long before this, and an entry that
/// fails to bind is still an adjustment entry for the purpose of cutting it.
pub(crate) fn is_adjustment(layer: &Layer) -> bool {
    layer.kind != LayerKind::Group
        && layer
            .meta
            .as_deref()
            .and_then(Adjustment::from_meta)
            .is_some()
}

/// `layers[i]` is above `idx` in the picture: past it in the bottom-first
/// order and not one of the groups that enclose it.
fn above(layers: &[Layer], idx: usize) -> Vec<bool> {
    let mut out = vec![false; layers.len()];
    for (i, flag) in out.iter_mut().enumerate() {
        *flag = i > idx;
    }
    let mut cursor = Some(idx);
    while let Some(i) = cursor {
        cursor = doc_group::parent(layers, i);
        if let Some(ancestor) = cursor {
            out[ancestor] = false;
        }
    }
    out
}

/// The entries contributing nothing to `mode`'s plate for target `idx`, or
/// `None` when the mode cuts nothing at all (the plate is then the document's
/// own projection, statement for statement).
///
/// The returned vector is exactly `layers.len()` long, so a [`Cut`] built from
/// it addresses every entry.
pub(crate) fn excluded(
    layers: &[Layer],
    idx: usize,
    mode: RetouchSampling,
    ignore_adjustments: bool,
) -> Option<Vec<bool>> {
    match mode {
        // No composite runs, so there is nothing to cut.
        RetouchSampling::Current => None,
        RetouchSampling::CurrentAndBelow => Some(above(layers, idx)),
        RetouchSampling::All if ignore_adjustments => {
            let mut cut = above(layers, idx);
            for (i, flag) in cut.iter_mut().enumerate() {
                *flag = *flag && is_adjustment(&layers[i]);
            }
            cut.iter().any(|&f| f).then_some(cut)
        }
        RetouchSampling::All => None,
    }
}

impl DrDocument {
    /// The cut `mode` derives for target `idx`, as an owned vector the caller
    /// borrows into a [`Cut`]. Split from [`excluded`] only so the layer slice
    /// and the document stay one call apart at the call sites.
    pub(crate) fn retouch_cut(
        &self,
        idx: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
    ) -> Option<Vec<bool>> {
        excluded(&self.layers, idx, mode, ignore_adjustments)
    }
}

impl DrDocument {
    /// Current → `layer_canvas_image(idx)` iff `raster_layer(idx)` is Some; else
    /// `flattened_cut(MAX_GROUP_BUFFER_PIXELS, cut_of(retouch_cut(..)))`. Test seam only:
    /// the full-frame reference a cut sampling plate is compared against
    /// (13A-5c-2b; the `render_region_with_group_budget` precedent).
    #[doc(hidden)]
    pub fn retouch_plate_full_frame(
        &self,
        idx: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
    ) -> Result<Option<RgbaImage>, String> {
        if mode.is_layer_only() {
            return Ok(self
                .raster_layer(idx)
                .is_some()
                .then(|| self.layer_canvas_image(idx))
                .flatten());
        }
        let excluded = self.retouch_cut(idx, mode, ignore_adjustments);
        self.flattened_cut(
            doc_group::MAX_GROUP_BUFFER_PIXELS,
            cut_of(excluded.as_ref()),
        )
        .map(Some)
    }
}

/// Borrow an optional cut vector as a [`Cut`]. One line, written once, so no
/// call site can accidentally pass an empty slice (which would cut nothing)
/// where it meant `Cut::NONE`.
pub(crate) fn cut_of(excluded: Option<&Vec<bool>>) -> Cut<'_> {
    match excluded {
        Some(e) => Cut::new(e),
        None => Cut::NONE,
    }
}
