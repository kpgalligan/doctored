//! Shared conservative admission estimate for existing full-frame rendering.
//! Retained document input allocations are accounted separately; caches may be
//! counted here as potential construction and by the retained inventory, so the
//! result is a safe reservation rather than an estimate of actual live usage.
//!
//! shares its conservative full-projection reservation with resource reporting and host admission;
//! potential style-cache construction is deliberately included even when cached planes already
//! exist.

use crate::doc::{DrDocument, LayerKind, MAX_PIXELS};

/// Conservative peak full-projection scratch, including the returned RGBA8. Full-frame group costs mirror doc_group's
/// documented 56-byte worst styled clip level. Style construction is reserved
/// separately over each padded extent at128bytes/pixel for new cached coverage,
/// EDT/blur scratch and backdrop snapshots; already cached planes may be counted
/// twice, deliberately conservative. This is an admission bound, not live usage.
pub(crate) fn projection_scratch_bytes(doc: &DrDocument) -> u64 {
    let canvas = u64::from(doc.width) * u64::from(doc.height);
    let mut bytes = canvas.saturating_mul(20);
    if doc.layers.iter().any(|l| l.kind == LayerKind::Group) {
        let depth = doc
            .layers
            .iter()
            .map(|l| u64::from(l.depth) + 1)
            .max()
            .unwrap_or(0);
        bytes = bytes.saturating_add(canvas.saturating_mul(56).saturating_mul(depth));
    } else if doc.layers.iter().any(|l| l.clipped) {
        bytes = bytes.saturating_add(canvas.saturating_mul(20));
    }
    if doc.layers.iter().any(|l| l.meta.is_some()) {
        bytes = bytes.saturating_add(canvas.saturating_mul(32));
    }
    for layer in &doc.layers {
        if let Some(mask) = &layer.mask {
            let p = &layer.mask_properties;
            if p.feather > 0.0 {
                bytes = bytes.saturating_add(p.scratch_bytes(mask.dimensions()));
            }
            if p.feather > 0.0
                || p.density != 1.0
                || p.offset != layer.offset
                || mask.dimensions() != layer.pixels.dimensions()
            {
                bytes = bytes.saturating_add(if layer.kind == LayerKind::Group {
                    canvas
                } else {
                    u64::from(layer.pixels.width()) * u64::from(layer.pixels.height())
                });
            }
        }
        // A STYLED adjustment is styled too: its shape is the
        // layer's own rect with an enabled mask and the canvas without
        // (`style_adjustment`), and a knockout or grouped interior's unit
        // pass holds its interior pixels there (`Adjusted::interior_layer`,
        // at most 4 B/px of RGBA8) beside the same per-plane charge.
        let Some(style) = layer.renders_any_style() else {
            continue;
        };
        let adjustment = layer.renders_style().is_none();
        let (w, h) = if layer.kind == LayerKind::Group
            || (adjustment && !(layer.mask.is_some() && layer.mask_enabled))
        {
            (doc.width, doc.height)
        } else {
            layer.pixels.dimensions()
        };
        if adjustment {
            bytes = bytes.saturating_add((u64::from(w) * u64::from(h)).saturating_mul(4));
        }
        let pad = u64::from(style.pad()) * 2;
        let pixels = (u64::from(w) + pad).saturating_mul(u64::from(h) + pad);
        if pixels <= MAX_PIXELS {
            bytes = bytes.saturating_add(pixels.saturating_mul(if style.has_enabled_effects() {
                128
            } else {
                16
            }));
        }
    }
    // The knockout backdrops the projection may snapshot (`style_knockout`,
    // D8): the Background, one per pass-through level that could hold a
    // Shallow child, and one per clip unit with a knockout member — 16 bytes
    // per accumulator pixel each. Zero for a document carrying no knockout,
    // which is what keeps every existing reservation exactly what it was.
    bytes = bytes.saturating_add(crate::style_knockout::snapshot_pixels(doc).saturating_mul(16));
    bytes
}
