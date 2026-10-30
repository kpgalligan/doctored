//! The record tree both mode conversions carry (#547 phase 4a and the
//! styles batch, `cmyk_convert`): groups (Normal and Pass Through, at any
//! depth), layer masks, clipping, blend modes, layer styles (effects,
//! knockout, Blend Interior, Fill) ride across Image ▸ Mode ▸
//! CMYK Color… and RGB Color… with every leaf's pixels converted and every
//! panel property (id, label, locks, link, disclosure) kept. A style's
//! colours cross as they are: toward CMYK the authored hex converts at
//! composite time (`cmyk_style`), toward RGB every ink is dropped
//! ([`without_inks`]).
//!
//! RGB → CMYK names its FIRST blocker in postorder (children before their
//! group) — what the CMYK mode cannot hold (`cmyk_document`): an RGB
//! adjustment layer, a Blend If on Red, Green or Blue, a blend mode outside `cmyk_document::CMYK_BLEND_MODES`, a placed image or a
//! warp. A text, shape or fill layer converts as its pixels, its description
//! dropped; the host re-renders it in the destination's colours (#547's
//! artwork batch). A Gray Blend If carries both ways ([`carry`]); its ramps
//! read the destination's own Gray. CMYK → RGB's blockers
//! ([`first_rgb_blocker`]) are a CMYK adjustment record and a Blend If on a
//! plate, anywhere in the tree. Either answer is the sentence the host
//! shows beside its "flatten the image" choice.

use crate::cmyk_document::{mode_name, CMYK_BLEND_MODES};
use crate::precision_document::{Content, PrecisionDocumentLayer};
use crate::style::BlendIfChannel;

/// The first record (postorder) the CMYK mode cannot hold, as a sentence
/// (module doc).
pub(crate) fn first_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    first_blocker_named(layers, &BlendIfChannel::display_name)
}

/// [`first_blocker`] with a Blend If channel named by `channel_name`: a Lab
/// document's Red, Green and Blue are Lightness, a and b
/// (`lab_cmyk::first_blocker`, #547 Lab phase 5a).
pub(crate) fn first_blocker_named(
    layers: &[PrecisionDocumentLayer],
    channel_name: &dyn Fn(BlendIfChannel) -> &'static str,
) -> Option<String> {
    for layer in layers {
        if let Some(reason) = layer
            .children()
            .and_then(|children| first_blocker_named(children, channel_name))
        {
            return Some(reason);
        }
        let mode = (!CMYK_BLEND_MODES.contains(&layer.blend_mode)).then(|| {
            format!(
                "a layer with the {} blend mode",
                mode_name(layer.blend_mode)
            )
        });
        let what = match &layer.content {
            Content::Adjustment(_) => Some("an adjustment layer".to_string()),
            _ => None,
        }
        .or(layer
            .blend_if
            .and_then(|b| b.ranges().find(|r| r.channel.is_rgb()))
            .map(|r| {
                format!(
                    "a layer with Blend If on the {} channel",
                    channel_name(r.channel)
                )
            }))
        .or(mode)
        .or(layer
            .retained_source
            .as_ref()
            .map(|_| "a placed image".to_string()))
        .or(layer.warp.as_ref().map(|_| "a warped layer".to_string()));
        if let Some(what) = what {
            return Some(format!(
                "“{}” is {what}, which a CMYK document cannot hold yet; flatten the image to convert it",
                layer.name
            ));
        }
    }
    None
}

/// The first record (postorder) RGB Color cannot carry — a CMYK adjustment
/// record or a Blend If on a plate — as a sentence (module doc).
pub(crate) fn first_rgb_blocker(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    for layer in layers {
        if let Some(reason) = layer.children().and_then(first_rgb_blocker) {
            return Some(reason);
        }
        let what = if matches!(layer.content, Content::CmykAdjustment(_)) {
            Some("a CMYK adjustment layer".to_string())
        } else {
            layer
                .blend_if
                .and_then(|b| b.ranges().find(|r| r.channel.is_cmyk()))
                .map(|r| {
                    format!(
                        "a layer with Blend If on the {} channel",
                        r.channel.display_name()
                    )
                })
        };
        if let Some(what) = what {
            return Some(format!(
                "“{}” is {what}, which RGB Color cannot carry; flatten the image to convert it",
                layer.name
            ));
        }
    }
    None
}

/// Every record's style without its CMYK inks (CMYK → RGB, module doc).
pub(crate) fn without_inks(layers: &mut [PrecisionDocumentLayer]) {
    for layer in layers.iter_mut() {
        if let Some(style) = layer.style.as_deref().filter(|s| s.has_inks()) {
            layer.style = Some(std::sync::Arc::new(style.without_inks()));
        }
        if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
            without_inks(std::sync::Arc::make_mut(children).as_mut_slice());
        }
    }
}

/// `out` with `from`'s id, label, reference point, locks, link, mask, blend
/// mode, clipping, style, Blend If and disclosure.
pub(crate) fn carry(
    from: &PrecisionDocumentLayer,
    mut out: PrecisionDocumentLayer,
) -> PrecisionDocumentLayer {
    out.mask = from.mask.clone();
    out.id = from.id;
    out.label = from.label;
    out.reference = from.reference;
    out.locks = from.locks;
    out.link = from.link;
    out.blend_mode = from.blend_mode;
    out.clipped = from.clipped;
    out.style = from.style.clone();
    out.blend_if = from.blend_if;
    out.open = from.open;
    out
}

/// `layers` with every leaf replaced by `leaf(layer)` and every group
/// rebuilt around its mapped children, each record [`carry`]ing its
/// properties (module doc).
pub(crate) fn map_tree(
    layers: &[PrecisionDocumentLayer],
    leaf: &mut dyn FnMut(&PrecisionDocumentLayer) -> Result<PrecisionDocumentLayer, String>,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    let mut out = Vec::with_capacity(layers.len());
    for layer in layers {
        let mapped = match layer.children() {
            Some(children) => PrecisionDocumentLayer::group(
                &layer.name,
                map_tree(children, leaf)?,
                layer.opacity,
                layer.visible,
                layer.is_pass_through(),
            )?,
            None => leaf(layer)?,
        };
        out.push(carry(layer, mapped));
    }
    Ok(out)
}

/// The leaves of `layers`, at every depth.
pub(crate) fn leaf_count(layers: &[PrecisionDocumentLayer]) -> usize {
    layers
        .iter()
        .map(|layer| layer.children().map_or(1, leaf_count))
        .sum()
}
