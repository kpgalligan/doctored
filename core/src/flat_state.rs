//! The ONE inventory of editable state a flattened file has no place for,
//! named in the words Save's loss list uses. A byte document
//! (`doc_preservation`) and a typed one (`precision_preservation`) each fill
//! an [`EditableState`] from their own records, and [`losses`] turns it into
//! the lines, in one order, so the two lists cannot drift apart. What a
//! format carries beside the editable state (profile, density, metadata
//! packets, transparency) stays with the caller that knows the format.

/// Which kinds of editable state a document holds, each `true` when a flat
/// file would drop it.
#[derive(Default)]
pub(crate) struct EditableState {
    /// An embedded source image or a retained transform.
    pub(crate) sources: bool,
    /// A persisted warp field and its pre-warp source.
    pub(crate) warps: bool,
    /// More than one layer, or a layer that is not a pixel layer (a group,
    /// an adjustment): a flat file holds the composite.
    pub(crate) layers: bool,
    pub(crate) masks: bool,
    /// A text, shape, fill or generated layer's re-openable description.
    pub(crate) descriptions: bool,
    /// A layer style, Fill Opacity or Blend If.
    pub(crate) styles: bool,
    /// A layer name other than the one the flat file reopens with, or a
    /// hidden, translucent, blended, clipped, locked, linked, labelled,
    /// nested or collapsed layer.
    pub(crate) properties: bool,
    /// A layer offset from the origin or not the canvas's size.
    pub(crate) bounds: bool,
    /// An alpha channel (a saved selection).
    pub(crate) channels: bool,
    /// A spot channel the format cannot keep (a TIFF keeps them as extra
    /// samples, `tiff_spot`).
    pub(crate) spot_channels: bool,
    /// A guide, or a ruler origin away from the canvas's top-left.
    pub(crate) guides: bool,
    pub(crate) samplers: bool,
    pub(crate) paths: bool,
    pub(crate) comps: bool,
    /// A global light other than the default 120°/30°.
    pub(crate) global_light: bool,
}

/// The lines of `state`, in the order Save reports them.
pub(crate) fn losses(state: &EditableState) -> Vec<&'static str> {
    [
        (
            state.sources,
            "embedded source images and retained transforms",
        ),
        (state.warps, "persisted warp field and pre-warp source"),
        (state.layers, "layers and groups"),
        (state.masks, "layer masks"),
        (state.descriptions, "editable layer descriptions"),
        (state.styles, "layer styles"),
        (state.properties, "layer properties"),
        (state.bounds, "layer bounds and off-canvas pixels"),
        (state.channels, "saved selection channels"),
        (state.spot_channels, crate::spot_channel::FLAT_LOSS),
        (state.guides, "guides and ruler origin"),
        (state.samplers, "color samplers"),
        (state.paths, "paths"),
        (state.comps, "layer comps"),
        (state.global_light, "global light settings"),
    ]
    .into_iter()
    .filter_map(|(lost, line)| lost.then_some(line))
    .collect()
}
