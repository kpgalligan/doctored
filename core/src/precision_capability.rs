//! The qualified precision operation/target/depth matrix. This is a family
//! admission decision, not a promise that every parameter/record fits: existing
//! strict operation planners still validate kinds, dimensions, profiles, work
//! and resources. Unknown operations, target aliases and depths fail closed.

use serde_json::{json, Value};

const FAMILIES: &[(&str, &[&str])] = &[
    ("source_place", &["document"]),
    ("source_transform", &["raster"]),
    ("source_rasterize", &["raster"]),
    ("source_replace", &["raster"]),
    ("adjustment_apply", &["raster"]),
    ("adjustment_add", &["document"]),
    ("adjustment_set", &["document"]),
    ("color_assign", &["document"]),
    ("color_convert", &["document"]),
    ("layer_duplicate", &["raster", "group", "adjustment"]),
    ("layer_blend", &["raster", "group", "adjustment"]),
    ("layer_opacity", &["raster", "group", "adjustment"]),
    ("layer_name", &["raster", "group", "adjustment"]),
    ("layer_visible", &["raster", "group", "adjustment"]),
    ("layer_clipped", &["raster", "group", "adjustment"]),
    ("layer_label", &["raster", "group", "adjustment"]),
    ("layer_merge", &["raster", "group", "adjustment"]),
    ("layer_merge_visible", &["document"]),
    ("layer_stamp", &["document"]),
    // #1 phase 4c: Stamp Selected (`precision_merge_selected`).
    ("layer_stamp_selected", &["raster", "group", "adjustment"]),
    ("layer_flatten", &["document"]),
    ("layer_lock", &["raster", "group", "adjustment"]),
    ("layer_link", &["raster", "group", "adjustment"]),
    ("layer_comp", &["document"]),
    // The typed global light, also in `FLOAT_FAMILIES`.
    ("global_light", &["document"]),
    // An RGB adjustment carries a style and Blend If.
    ("layer_blend_if", &["raster", "group", "adjustment"]),
    ("layer_style", &["raster", "group", "adjustment"]),
    ("layer_move", &["raster", "group", "adjustment"]),
    ("layer_group", &["raster", "group", "adjustment"]),
    ("layer_ungroup", &["group"]),
    ("layer_remove", &["raster", "group", "adjustment"]),
    ("layer_add", &["document"]),
    // Layer Via Copy / Cut (`precision_layer_via`), also in `FLOAT_FAMILIES`.
    ("layer_via", &["raster"]),
    // Stack Modes (`precision_stack_mode`), 16 bits and float alike.
    ("layer_stack_mode", &["raster"]),
    ("geometry_affine", &["raster", "mask"]),
    ("geometry_perspective", &["raster", "mask"]),
    ("geometry_warp", &["raster", "mask"]),
    // Edit ▸ Puppet Warp on a layer's pixels (`precision_puppet`).
    ("geometry_puppet", &["raster"]),
    // Edit ▸ Perspective Warp on a layer's pixels
    // (`precision_perspective_warp`).
    ("geometry_perspective_warp", &["raster"]),
    // Set geometry expands a group/adjustment root to its editable content.
    // The historical leaf families above keep their narrower contract.
    ("geometry_affine_set", &["raster", "group", "adjustment"]),
    (
        "geometry_perspective_set",
        &["raster", "group", "adjustment"],
    ),
    ("geometry_resize", &["document"]),
    ("geometry_crop", &["document"]),
    // Phase 12F-1b2: STRAIGHTEN is a crop with a non-zero angle and a family
    // of its own, because the crop family cannot tell the two apart and only
    // straighten resamples. Also in `FLOAT_FAMILIES`.
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("mask_set", &["raster", "group", "adjustment"]),
    ("mask_remove", &["mask"]),
    ("mask_properties", &["mask"]),
    ("mask_transform", &["mask"]),
    ("channel_set", &["document", "channel"]),
    ("channel_remove", &["channel"]),
    ("channel_read", &["channel"]),
    (
        "read_plane",
        &[
            "mask",
            "effective_mask",
            "channel",
            "red",
            "green",
            "blue",
            "alpha",
        ],
    ),
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    ("clone", &["raster"]),
    ("heal", &["raster"]),
    // Move Selected Pixels, also of the selected area of a targeted mask or
    // alpha channel or of one colour channel of the layer. Its float row
    // (layer pixels) is in `FLOAT_FAMILIES`.
    (
        "selected_pixels_move",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
    // Free Transform of the selected pixels, also of the selected area of a
    // targeted mask or alpha channel or of one colour channel of the layer.
    // Its float row (layer pixels) is in `FLOAT_FAMILIES`.
    (
        "selected_pixels_transform",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
    ("rasterize", &["document"]),
    ("sample", &["document", "raster"]),
    ("histogram", &["document", "raster"]),
    ("render", &["document"]),
    ("convert", &["document"]),
    ("export", &["document"]),
    ("clipboard_copy", &["raster", "group"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("clipboard_paste", &["document"]),
    (
        "fill_stroke",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
];

/// The float document's own admitted families (Phase 11F): the depth-32
/// table. Everything outside it refuses by name — the same bounded set
/// `float_document`'s operator table states.
const FLOAT_FAMILIES: &[(&str, &[&str])] = &[
    ("adjustment_add", &["document"]),
    ("adjustment_set", &["document"]),
    ("layer_duplicate", &["raster", "group", "adjustment"]),
    ("layer_blend", &["raster", "group", "adjustment"]),
    ("layer_opacity", &["raster", "group", "adjustment"]),
    ("layer_name", &["raster", "group", "adjustment"]),
    ("layer_visible", &["raster", "group", "adjustment"]),
    ("layer_clipped", &["raster", "group", "adjustment"]),
    ("layer_label", &["raster", "group", "adjustment"]),
    ("layer_merge", &["raster", "group", "adjustment"]),
    ("layer_merge_visible", &["document"]),
    ("layer_stamp", &["document"]),
    // #1 phase 4c: Stamp Selected's f32 arm (`precision_merge_selected`).
    ("layer_stamp_selected", &["raster", "group", "adjustment"]),
    ("layer_flatten", &["document"]),
    ("layer_lock", &["raster", "group", "adjustment"]),
    ("layer_link", &["raster", "group", "adjustment"]),
    ("layer_comp", &["document"]),
    ("geometry_canvas_size", &["document"]),
    // Phase 12F-1b: the EXACT canvas ops. A zero-angle crop keeps every
    // allocation; the quarter turns and the flips are a Nearest permutation.
    // The FILTERED families resample premultiplied extended-range f32
    // and clamp RGB to the layer's own visible range (`float_document`'s
    // D3). `geometry_warp` takes the same finish; a float warp keeps
    // no re-editable field, so each set warps the current pixels again.
    ("geometry_crop", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("geometry_resize", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("geometry_affine", &["raster", "mask"]),
    ("geometry_perspective", &["raster", "mask"]),
    ("geometry_warp", &["raster", "mask"]),
    // Puppet Warp's f32 arm (`precision_puppet`, D3 as the warp).
    ("geometry_puppet", &["raster"]),
    // Perspective Warp's f32 arm (`precision_perspective_warp`, D3).
    ("geometry_perspective_warp", &["raster"]),
    ("geometry_affine_set", &["raster", "group", "adjustment"]),
    (
        "geometry_perspective_set",
        &["raster", "group", "adjustment"],
    ),
    ("layer_move", &["raster", "group", "adjustment"]),
    ("layer_group", &["raster", "group", "adjustment"]),
    ("layer_ungroup", &["group"]),
    ("layer_remove", &["raster", "group", "adjustment"]),
    ("layer_add", &["document"]),
    // Layer Via Copy / Cut on a float layer (`precision_layer_via`'s f32 arm).
    ("layer_via", &["raster"]),
    // Stack Modes on float layers (`precision_stack_mode`'s f32 arm).
    ("layer_stack_mode", &["raster"]),
    // Assign Profile relabels the linear samples (`float_color`).
    ("color_assign", &["document"]),
    // Convert to Profile in linear light with no clamp (`float_color`).
    ("color_convert", &["document"]),
    ("mask_set", &["raster", "group", "adjustment"]),
    ("mask_remove", &["mask"]),
    ("mask_properties", &["mask"]),
    ("mask_transform", &["mask"]),
    ("channel_set", &["document", "channel"]),
    ("channel_remove", &["channel"]),
    ("channel_read", &["channel"]),
    ("read_plane", &["mask", "effective_mask", "channel"]),
    ("sample", &["document", "raster"]),
    ("histogram", &["document", "raster"]),
    ("render", &["document"]),
    ("convert", &["document"]),
    ("export", &["document"]),
    ("clipboard_copy", &["raster", "group"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("clipboard_paste", &["document"]),
    // Fill…/Stroke… on a layer, its mask or an alpha channel
    // (`float_paint`), and one colour channel of a float layer, the
    // `plane` of the same `paint_blend` (Photoshop's measured 30/59/11
    // value, `plane_swatch::plane_fill_value`).
    (
        "fill_stroke",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
    // The Brush, Gradient, Paint Bucket and Dodge/Burn (`paint`) and
    // the Eraser (`erase`) on a float layer (`float_paint`); a mask or
    // channel is u16 coverage.
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    // The Clone Stamp (`clone`) and the healing family (`heal`: the
    // Healing Brush, Spot Healing, Patch, Remove, Content-Aware Fill and
    // Move) on a float layer's pixels (`float_retouch`).
    ("clone", &["raster"]),
    ("heal", &["raster"]),
    // The Blur, Sharpen, Sponge and Smudge brushes and the History
    // Brush on a float layer's pixels (`float_retouch_brush`).
    ("retouch_brush", &["raster"]),
    // The selected area of a mask or alpha channel (u16 planes at every
    // depth). The move of the same planes is the transform's integer
    // translation through the same walk. A float LAYER's selected pixels go
    // through the same walk at f32, a filtered transform landing under
    // `float_document`'s D3 clamp, and so does one colour channel of a float
    // layer, its vacated value unconverted (take2's F-RAW).
    (
        "selected_pixels_transform",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
    (
        "selected_pixels_move",
        &["raster", "mask", "channel", "red", "green", "blue", "alpha"],
    ),
    // Layer styles, Blend If and the light they read, each
    // measured in Photoshop 26.10.0 (`style_float`); a style part the float
    // mode cannot hold refuses by name (`float_document::style_refusal`).
    ("global_light", &["document"]),
    // An RGB adjustment carries a style and Blend If.
    ("layer_blend_if", &["raster", "group", "adjustment"]),
    ("layer_style", &["raster", "group", "adjustment"]),
    // Destructive Image ▸ Adjustments on a layer's pixels,
    // the layer's own float forms (`float_adjust_apply`); the op itself is
    // admitted or refused by name there.
    ("adjustment_apply", &["raster"]),
    // Placed images (in the app, RAW layers), rendered into
    // linear light from their 8/16-bit master (`retained_source_float`).
    ("source_place", &["document"]),
    ("source_transform", &["raster"]),
    ("source_rasterize", &["raster"]),
    ("source_replace", &["raster"]),
    // Text and shape layers, rendered into linear light by
    // `artwork_render::render_float` (Photoshop 26.10.0's measured 32-bit
    // type and shape colour, coverage and mixing).
    ("rasterize", &["document"]),
];

/// The CMYK document's own admitted families (#1 phases 1, 2, 4a, 4b, 4c and 5,
/// `cmyk_document`'s operation table). Everything outside it refuses by
/// name through `cmyk_document::refused_sentence`.
const CMYK_FAMILIES: &[(&str, &[&str])] = &[
    ("layer_duplicate", &["raster", "adjustment"]),
    ("layer_remove", &["raster", "adjustment"]),
    ("layer_move", &["raster", "adjustment"]),
    ("layer_name", &["raster", "adjustment"]),
    ("layer_visible", &["raster", "adjustment"]),
    ("layer_opacity", &["raster", "adjustment"]),
    ("layer_label", &["raster", "adjustment"]),
    ("layer_lock", &["raster", "adjustment"]),
    ("layer_flatten", &["document"]),
    ("render", &["document"]),
    ("sample", &["document"]),
    ("export", &["document"]),
    ("convert", &["document"]),
    // Phase 2: New Layer, the ink tools, masks and Levels/Curves.
    ("layer_add", &["document"]),
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    ("fill_stroke", &["raster", "mask"]),
    ("mask_set", &["raster", "adjustment"]),
    ("mask_remove", &["mask"]),
    ("mask_properties", &["mask"]),
    ("adjustment_add", &["document"]),
    ("adjustment_set", &["document"]),
    ("adjustment_apply", &["raster"]),
    ("read_plane", &["mask", "effective_mask"]),
    // The selection batch: source selections, Apply Layer Mask.
    ("selection", &["document", "raster"]),
    ("mask_apply", &["mask"]),
    // Phase 4a: groups, clipping masks, blend modes and Fill Opacity,
    // merges and Stamp Visible. A family may take more than one line; its
    // targets are every line's.
    ("layer_group", &["raster", "group", "adjustment"]),
    ("layer_ungroup", &["group"]),
    ("layer_blend", &["raster", "group", "adjustment"]),
    ("layer_clipped", &["raster", "group", "adjustment"]),
    ("layer_merge", &["raster", "group", "adjustment"]),
    ("layer_merge_visible", &["document"]),
    ("layer_stamp", &["document"]),
    // An adjustment too (`cmyk_style_row`'s adjustment interior).
    ("layer_style", &["raster", "group", "adjustment"]),
    // The Blend If batch: Gray and the four plates (`cmyk_document::style_refusal`).
    ("layer_blend_if", &["raster", "group", "adjustment"]),
    ("layer_duplicate", &["group"]),
    ("layer_remove", &["group"]),
    ("layer_move", &["group"]),
    ("layer_name", &["group"]),
    ("layer_visible", &["group"]),
    ("layer_opacity", &["group"]),
    ("layer_label", &["group"]),
    ("layer_lock", &["group"]),
    ("mask_set", &["group"]),
    // Phase 5: Assign/Convert to Profile and 8/16 Bits, the CMYK proof, and
    // Levels/Curves' histogram, Auto and eyedroppers.
    ("profile", &["document"]),
    ("proof", &["document"]),
    ("analysis", &["document"]),
    // Phase 4b: geometry on the ink and its masks (`precision_geometry_cmyk`,
    // `cmyk_warp`, `precision_puppet`'s CMYK arm), the selected pixels of a
    // layer or its mask (`cmyk_selected_pixels`) and Transform Mask.
    ("geometry_affine", &["raster", "mask"]),
    ("geometry_perspective", &["raster", "mask"]),
    ("geometry_warp", &["raster", "mask"]),
    ("geometry_puppet", &["raster"]),
    ("geometry_perspective_warp", &["raster"]),
    ("geometry_affine_set", &["raster", "group", "adjustment"]),
    (
        "geometry_perspective_set",
        &["raster", "group", "adjustment"],
    ),
    ("geometry_resize", &["document"]),
    ("geometry_crop", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("mask_transform", &["mask"]),
    ("selected_pixels_move", &["raster", "mask"]),
    ("selected_pixels_transform", &["raster", "mask"]),
    // Phase 4c: copy and paste, Layer Via, Stamp Selected.
    ("clipboard_copy", &["raster", "group"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("clipboard_paste", &["document"]),
    ("layer_via", &["raster"]),
    ("layer_stamp_selected", &["raster", "group", "adjustment"]),
    // The channels batch: alpha channels (New/Duplicate/Delete/Options,
    // Save/Load Selection, painting and filling one), reading one, and the
    // selected pixels of an alpha channel or of one ink plate
    // (`cmyk_selected_plate`).
    ("channel_set", &["document", "channel"]),
    ("channel_remove", &["channel"]),
    ("channel_read", &["channel"]),
    ("read_plane", &["channel"]),
    ("fill_stroke", &["channel"]),
    ("selected_pixels_move", &["channel"]),
    ("selected_pixels_transform", &["channel"]),
    (
        "selected_pixels_move",
        &["cyan", "magenta", "yellow", "black"],
    ),
    (
        "selected_pixels_transform",
        &["cyan", "magenta", "yellow", "black"],
    ),
    // #1's retouch batch: the Clone Stamp, the healing family and the
    // Blur, Sharpen, Sponge, Smudge and History brushes on ink.
    ("clone", &["raster"]),
    ("heal", &["raster"]),
    ("retouch_brush", &["raster"]),
    // #1's artwork batch: a new type, shape or fill layer in ink
    // (`cmyk_artwork`).
    ("rasterize", &["document"]),
    // The styles batch: the document's global light (`cmyk_style`).
    ("global_light", &["document"]),
    // The remaining-gaps batch: one ink plate as a scalar target
    // (`cmyk_plate_scalar`), and adjusting an alpha channel.
    ("fill_stroke", &["cyan", "magenta", "yellow", "black"]),
    ("read_plane", &["cyan", "magenta", "yellow", "black"]),
    ("paint", &["cyan", "magenta", "yellow", "black"]),
    ("erase", &["cyan", "magenta", "yellow", "black"]),
    ("adjustment_apply", &["cyan", "magenta", "yellow", "black"]),
    ("adjustment_apply", &["channel"]),
    // Stack Modes per ink, over the `L` passes (`cmyk_stack_mode`).
    ("layer_stack_mode", &["raster"]),
];

/// The CMYK table's decision for one family and target: admitted when ANY
/// line of the family lists the target.
pub fn require_cmyk(operation: &str, target: &str) -> Result<(), String> {
    let mut lines = CMYK_FAMILIES
        .iter()
        .filter(|(name, _)| *name == operation)
        .peekable();
    if lines.peek().is_none() {
        return Err(crate::cmyk_document::refused_sentence(operation));
    }
    if !lines.any(|(_, targets)| targets.contains(&target)) {
        return Err(format!("CMYK {operation} does not support target {target}"));
    }
    Ok(())
}

/// The CMYK table's family names (the descriptor's `supported_operations`),
/// each once, in table order.
pub(crate) fn cmyk_operations() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (name, _) in CMYK_FAMILIES {
        if !out.contains(name) {
            out.push(name);
        }
    }
    out
}

/// The Grayscale document's admitted families (#1, `gray_document`):
/// phase 1a's structure families and the document's flatten, render, sample
/// and mode conversion; phase 1b's layer structure (New Layer, groups,
/// blend modes, clipping, merges, Stamp Visible, Fill Opacity, layer masks),
/// Fill and the one-channel export; phase 2a-ii's paint and erase, phase
/// 2b's adjustments, an adjustment layer on the layer rows, phase 2c's
/// clone and heal (the retouch tools on R = G = B, `gray_filter`), and phase
/// 2d's Blend If, global light and alpha channels (structure, and a paint,
/// Fill, filter and read target wherever the mask is one; `gray_style`), and
/// phase 4's geometry, warps, selected pixels, clipboard, Layer Via, Stamp
/// Selected and selections (the RGB kernels on R = G = B; `gray_clipboard`),
/// and phase 5a's profile edits (Assign and Convert to Profile, Bits/Channel;
/// `gray_profile`), and phase 5b's proof (the RGB proof of the composite on
/// R = G = B in the twin), and phase 6a-i's new type, shape or fill layer
/// (`gray_artwork`), and phase 6a-ii's links and layer comps (`gray_comps`;
/// the legacy paint ops are the `paint`, `erase`, `clone` and `heal` lines
/// above), and phase 6b's Histogram and Stack Modes, and the close's Place
/// (its rendered pixels folded to gray, no retained source kept). Every
/// other family is one Doctored offers on RGB documents only, and refuses
/// for good by name through `gray_document::refused_sentence`.
const GRAY_FAMILIES: &[(&str, &[&str])] = &[
    ("layer_duplicate", &["raster", "group", "adjustment"]),
    ("layer_remove", &["raster", "group", "adjustment"]),
    ("layer_move", &["raster", "group", "adjustment"]),
    ("layer_name", &["raster", "group", "adjustment"]),
    ("layer_visible", &["raster", "group", "adjustment"]),
    ("layer_opacity", &["raster", "group", "adjustment"]),
    ("layer_label", &["raster", "group", "adjustment"]),
    ("layer_lock", &["raster", "group", "adjustment"]),
    ("layer_flatten", &["document"]),
    ("render", &["document"]),
    ("sample", &["document"]),
    ("convert", &["document"]),
    // Phase 1b.
    ("layer_add", &["document"]),
    ("layer_group", &["raster", "group", "adjustment"]),
    ("layer_ungroup", &["group"]),
    ("layer_blend", &["raster", "group", "adjustment"]),
    ("layer_clipped", &["raster", "group", "adjustment"]),
    ("layer_merge", &["raster", "group", "adjustment"]),
    ("layer_style", &["raster", "group", "adjustment"]),
    ("layer_merge_visible", &["document"]),
    ("layer_stamp", &["document"]),
    ("export", &["document"]),
    ("mask_set", &["raster", "group", "adjustment"]),
    ("mask_remove", &["mask"]),
    ("mask_properties", &["mask"]),
    ("mask_apply", &["mask"]),
    ("read_plane", &["mask", "channel"]),
    ("fill_stroke", &["raster", "mask", "channel"]),
    // Phase 2a-ii: the paint tools; phase 2b: the adjustments
    // (`gray_adjust`'s table decides each op).
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    ("adjustment_apply", &["raster", "channel"]),
    ("adjustment_add", &["document"]),
    ("adjustment_set", &["document"]),
    // Phase 2c: the retouch tools.
    ("clone", &["raster"]),
    ("heal", &["raster"]),
    // Phase 2d: Blend If, the global light and the alpha channels.
    ("layer_blend_if", &["raster", "group", "adjustment"]),
    ("global_light", &["document"]),
    ("channel_set", &["document", "channel"]),
    ("channel_remove", &["channel"]),
    ("channel_read", &["channel"]),
    // Phase 4: geometry, warps and Transform Mask on R = G = B through the
    // RGB kernels, the selected pixels of a layer, its mask or an alpha
    // channel, copy and paste, Layer Via, Stamp Selected and the selections.
    ("geometry_affine", &["raster", "mask"]),
    ("geometry_perspective", &["raster", "mask"]),
    ("geometry_warp", &["raster", "mask"]),
    ("geometry_puppet", &["raster"]),
    ("geometry_perspective_warp", &["raster"]),
    ("geometry_affine_set", &["raster", "group", "adjustment"]),
    (
        "geometry_perspective_set",
        &["raster", "group", "adjustment"],
    ),
    ("geometry_resize", &["document"]),
    ("geometry_crop", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("mask_transform", &["mask"]),
    ("selected_pixels_move", &["raster", "mask", "channel"]),
    ("selected_pixels_transform", &["raster", "mask", "channel"]),
    ("clipboard_copy", &["raster", "group"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("clipboard_paste", &["document"]),
    ("layer_via", &["raster"]),
    ("layer_stamp_selected", &["raster", "group", "adjustment"]),
    ("selection", &["document", "raster"]),
    // Phase 5a: Assign and Convert to Profile and Bits/Channel.
    ("profile", &["document"]),
    // Phase 5b: Proof Setup and Proof Colors (the RGB proof on R = G = B).
    ("proof", &["document"]),
    // Phase 6a-i: a new type, shape or fill layer.
    ("rasterize", &["document"]),
    // Phase 6a-ii: links and layer comps.
    ("layer_link", &["raster", "group", "adjustment"]),
    ("layer_comp", &["document"]),
    // Phase 6b: the Histogram and Stack Modes (the RGB kernels on R = G = B).
    ("histogram", &["document", "raster"]),
    ("layer_stack_mode", &["raster"]),
    // The close: Place, folded to gray with no retained source
    // (`precision_retained_source`'s Gray arm).
    ("source_place", &["document"]),
];

/// The Gray table's decision for one family and target.
pub fn require_gray(operation: &str, target: &str) -> Result<(), String> {
    let targets = GRAY_FAMILIES
        .iter()
        .find(|(name, _)| *name == operation)
        .map(|(_, targets)| *targets)
        .ok_or_else(|| crate::gray_document::refused_sentence(operation))?;
    if !targets.contains(&target) {
        return Err(format!(
            "Grayscale {operation} does not support target {target}"
        ));
    }
    Ok(())
}

/// The Gray table's family names (the descriptor's `supported_operations`),
/// in table order.
pub(crate) fn gray_operations() -> Vec<&'static str> {
    GRAY_FAMILIES.iter().map(|(name, _)| *name).collect()
}

/// The Lab Color document's admitted families (#1, `lab_document`): phase
/// 1's structure families on rasters and groups, and the document's
/// flatten, render and mode conversion; phase 1b's layer structure (New
/// Layer, groups, blend modes, clipping, merges, Stamp Visible, Fill
/// Opacity, layer masks), Fill, the export through RGB and the alpha
/// channels; phase 1c's document `sample` (`sample_color`/`sample_pixel`);
/// phase 2a's paint and erase, phase 2b's adjustments and an adjustment
/// layer on the layer rows; phase 2e's clone and heal, Global Light and an
/// alpha channel's read; phase 2f's Blend If; phase 4a's resamples and
/// transforms (the RGB kernels on the codes) and Transform Mask; phase
/// 4a-ii's Warp, Puppet Warp and Perspective Warp (the RGB kernels on the
/// codes, applied at once); phase 4b-i's selected pixels, Layer via, Stamp
/// Selected and source selections (the RGB kernels on the codes); phase
/// 4c's clipboard (`lab_clipboard`: the Lab payload, converted across
/// modes) and 5b's proof (the RGB proof of the document's Display P3 view,
/// `lab_convert::lab_display`; the host proofs it, and `soft_proof` refuses
/// the twin itself as a document profile); phase 6b-i's histogram and Stack
/// Modes (per channel on the codes).
/// Phase 6a-i's new type, shape or fill layer (`lab_artwork`: the one
/// renderer into the twin); phase 6a-ii's links, layer comps (`lab_comps`)
/// and the Lightness, a, b and alpha planes' selected pixels (the
/// colour-channel walk on the codes). The close's Place (`source_place`:
/// the ProPhoto render published as Lab codes, no retained source).
/// Everything outside it refuses by name through
/// `lab_document::refused_sentence`.
const LAB_FAMILIES: &[(&str, &[&str])] = &[
    ("layer_duplicate", &["raster", "group", "adjustment"]),
    ("layer_remove", &["raster", "group", "adjustment"]),
    ("layer_move", &["raster", "group", "adjustment"]),
    ("layer_name", &["raster", "group", "adjustment"]),
    ("layer_visible", &["raster", "group", "adjustment"]),
    ("layer_opacity", &["raster", "group", "adjustment"]),
    ("layer_label", &["raster", "group", "adjustment"]),
    ("layer_lock", &["raster", "group", "adjustment"]),
    ("layer_flatten", &["document"]),
    ("render", &["document"]),
    ("convert", &["document"]),
    // Phase 1b.
    ("layer_add", &["document"]),
    ("layer_merge_visible", &["document"]),
    ("layer_stamp", &["document"]),
    ("export", &["document"]),
    ("layer_group", &["raster", "group", "adjustment"]),
    ("layer_blend", &["raster", "group", "adjustment"]),
    ("layer_clipped", &["raster", "group", "adjustment"]),
    ("layer_merge", &["raster", "group", "adjustment"]),
    ("layer_style", &["raster", "group", "adjustment"]),
    ("mask_set", &["raster", "group", "adjustment"]),
    ("layer_ungroup", &["group"]),
    ("mask_remove", &["mask"]),
    ("mask_properties", &["mask"]),
    ("mask_apply", &["mask"]),
    ("read_plane", &["mask", "channel"]),
    ("fill_stroke", &["raster", "mask", "channel"]),
    ("channel_set", &["document", "channel"]),
    ("channel_remove", &["channel"]),
    // Phase 1c.
    ("sample", &["document"]),
    // Phase 2a: the paint tools; phase 2b: the adjustments
    // (`lab_adjust`'s table decides each op).
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    // Phase 2d: a mask's or alpha channel's filters and adjustments
    // (`lab_filter::scalar_refusal`), as Gray 2d's row.
    ("adjustment_apply", &["raster", "channel"]),
    ("adjustment_add", &["document"]),
    ("adjustment_set", &["document"]),
    // Phase 2e: the retouch tools (`lab_retouch`), Global Light for the
    // admitted effects, and an alpha channel's plane read.
    ("clone", &["raster"]),
    ("heal", &["raster"]),
    ("global_light", &["document"]),
    ("channel_read", &["channel"]),
    // Phase 2f: Blend If on Lightness, a and b (`lab_style`).
    ("layer_blend_if", &["raster", "group", "adjustment"]),
    // Phase 4a: the resamples and transforms, the RGB kernels on the
    // L*a*b* codes, and Transform Mask.
    ("geometry_affine", &["raster", "mask"]),
    ("geometry_perspective", &["raster", "mask"]),
    ("geometry_affine_set", &["raster", "group", "adjustment"]),
    (
        "geometry_perspective_set",
        &["raster", "group", "adjustment"],
    ),
    ("geometry_resize", &["document"]),
    ("geometry_crop", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("mask_transform", &["mask"]),
    // Phase 4a-ii: the warps, the RGB kernels on the L*a*b* codes, applied
    // at once with no binding (`precision_warp`).
    ("geometry_warp", &["raster", "mask"]),
    ("geometry_puppet", &["raster"]),
    ("geometry_perspective_warp", &["raster"]),
    // Phase 4b-i: the selected pixels, Layer via Copy and Cut, Stamp
    // Selected and the source selections, the RGB kernels on the L*a*b*
    // codes (`lab_document::admit_edit` refuses `luminance`, and Similar and
    // Grow at 8 bits; a colour plane target is admitted since phase 6b-i).
    ("selected_pixels_move", &["raster", "mask", "channel"]),
    ("selected_pixels_transform", &["raster", "mask", "channel"]),
    ("layer_via", &["raster"]),
    ("layer_stamp_selected", &["raster", "group", "adjustment"]),
    ("selection", &["document", "raster"]),
    // Phase 4c: the clipboard, its payload the Lab codes (`lab_clipboard`).
    ("clipboard_copy", &["raster", "group"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("clipboard_paste", &["document"]),
    // Phase 5b: Proof Setup and Proof Colors (the RGB proof of the Display
    // P3 view).
    ("proof", &["document"]),
    // Phase 6a-i: a new type, shape or fill layer (`lab_artwork`).
    ("rasterize", &["document"]),
    // Phase 6b-i: the histogram of the L*a*b* codes and Stack Modes, per
    // channel on the codes (`stack_stat`).
    ("histogram", &["document", "raster"]),
    ("layer_stack_mode", &["raster"]),
    // Phase 6a-ii: links and layer comps (`lab_comps`).
    ("layer_link", &["raster", "group", "adjustment"]),
    ("layer_comp", &["document"]),
    // The close: Place, rendered into ProPhoto RGB and published as Lab
    // codes with no retained source (`precision_retained_source`).
    ("source_place", &["document"]),
];

/// The Lab table's family names (the descriptor's `supported_operations`),
/// in table order.
pub(crate) fn lab_operations() -> Vec<&'static str> {
    LAB_FAMILIES.iter().map(|(name, _)| *name).collect()
}

/// The Indexed Color document's admitted families (#1 Indexed phase 1a,
/// `indexed_document`): the document's render, its `sample_color` and
/// `sample_pixel` readouts (the RGB storage through the scoped-analysis
/// path, not the edit request API), its mode conversion, (phase 1b) its
/// export, which keeps the palette for PNG, GIF and TIFF (`indexed_export`),
/// (phase 2b) the Brush in Normal mode on its one raster layer
/// (`indexed_paint`), (phase 2c) the Eraser, Fill and Stroke, Copy, Cut
/// and Copy Merged and the whole-canvas geometry (`indexed_paint`,
/// `indexed_geometry`), and (phase 2d) Paste, the layer transforms, the
/// selected pixels' transform, Warp and the layer row's name, label and lock
/// (`indexed_clipboard`, `indexed_geometry`, `indexed_transform`), and
/// (phase 3a) the selected pixels' move, both of an alpha channel too, the
/// layer row's visibility and order and Assign and Convert to Profile
/// (`indexed_transform`, `indexed_profile`), and (phase 3b) the paint in
/// every blend mode, the source selections that sample palette colours, an
/// alpha channel set and its plane read, and (phase 3c) the alpha channels'
/// commands and reads, Clone Stamp and the histogram.
/// Everything else refuses by name through
/// `indexed_document::refused_sentence`.
const INDEXED_FAMILIES: &[(&str, &[&str])] = &[
    ("render", &["document"]),
    ("sample", &["document"]),
    ("convert", &["document"]),
    ("export", &["document"]),
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    ("fill_stroke", &["raster"]),
    ("clipboard_copy", &["raster"]),
    ("clipboard_cut", &["raster"]),
    ("clipboard_copy_merged", &["document"]),
    ("geometry_crop", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("geometry_resize", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_rotate_canvas", &["document"]),
    ("clipboard_paste", &["document"]),
    ("geometry_affine", &["raster"]),
    ("geometry_perspective", &["raster"]),
    ("geometry_affine_set", &["raster"]),
    ("geometry_perspective_set", &["raster"]),
    ("selected_pixels_transform", &["raster", "channel"]),
    ("geometry_warp", &["raster"]),
    ("layer_name", &["raster"]),
    ("layer_label", &["raster"]),
    ("layer_lock", &["raster"]),
    ("selected_pixels_move", &["raster", "channel"]),
    ("layer_visible", &["raster"]),
    ("layer_move", &["raster"]),
    ("color_assign", &["document"]),
    ("color_convert", &["document"]),
    ("selection", &["document", "raster"]),
    ("channel_set", &["document", "channel"]),
    ("read_plane", &["channel"]),
    ("channel_remove", &["channel"]),
    ("channel_read", &["channel"]),
    ("clone", &["raster"]),
    ("histogram", &["document"]),
];

/// The Indexed table's family names (the descriptor's
/// `supported_operations`), in table order.
pub(crate) fn indexed_operations() -> Vec<&'static str> {
    INDEXED_FAMILIES.iter().map(|(name, _)| *name).collect()
}

/// The Bitmap document's admitted families (#1 Bitmap phase 1a,
/// `bitmap_document`): the document's render, its `sample_color` and
/// `sample_pixel` readouts, its mode conversion (to Grayscale) and, since
/// phase 1c, its export (the kept 1-bit PNG, TIFF and BMP of
/// `bitmap_export`, the PSD and PSB of `bitmap_psd`, every other format
/// through the Gray storage) and, since phase 2a-i (`bitmap_edit`), the
/// Brush, Eraser, Fill, Stroke, the Paint Bucket and Clear on the one layer
/// and the bit-exact geometry: Crop, Canvas Size, the quarter turns and the
/// flips, and since phase 2a-ii Cut, the resampled geometry (Image Size,
/// the Crop tool's straighten, the layer's affine and perspective
/// transforms and their sets) and the selected pixels' moves on the layer;
/// phase 2b took the selected pixels' transforms and arbitrary rotation
/// (Image ▸ Rotate Arbitrary…) out; phase 3 adds Copy, the layer row's
/// name, label and lock, the source selections, the Clone Stamp and the
/// histogram. Every other edit
/// refuses by name through `bitmap_document::refused_sentence`.
const BITMAP_FAMILIES: &[(&str, &[&str])] = &[
    ("render", &["document"]),
    ("sample", &["document"]),
    ("convert", &["document"]),
    ("export", &["document"]),
    ("paint", &["raster"]),
    ("erase", &["raster"]),
    ("fill_stroke", &["raster"]),
    ("geometry_crop", &["document"]),
    ("geometry_canvas_size", &["document"]),
    ("geometry_rotate", &["document"]),
    ("geometry_flip", &["document"]),
    ("clipboard_cut", &["raster"]),
    ("geometry_resize", &["document"]),
    ("geometry_straighten", &["document"]),
    ("geometry_affine", &["raster"]),
    ("geometry_perspective", &["raster"]),
    ("geometry_affine_set", &["raster"]),
    ("geometry_perspective_set", &["raster"]),
    ("selected_pixels_move", &["raster"]),
    ("clipboard_copy", &["raster"]),
    ("layer_name", &["raster"]),
    ("layer_label", &["raster"]),
    ("layer_lock", &["raster"]),
    ("selection", &["document", "raster"]),
    ("clone", &["raster"]),
    ("histogram", &["document"]),
];

/// The Bitmap table's family names (the descriptor's
/// `supported_operations`), in table order.
pub(crate) fn bitmap_operations() -> Vec<&'static str> {
    BITMAP_FAMILIES.iter().map(|(name, _)| *name).collect()
}

pub fn require(operation: &str, target: &str, depth: u32) -> Result<(), String> {
    if depth == 32 {
        let targets = FLOAT_FAMILIES
            .iter()
            .find(|(name, _)| *name == operation)
            .map(|(_, targets)| *targets)
            .ok_or_else(|| crate::float_document::refused_sentence(operation))?;
        if !targets.contains(&target) {
            return Err(format!(
                "32-bit float {operation} does not support target {target}"
            ));
        }
        return Ok(());
    }
    if depth != 16 {
        return Err("precision operations require the rgba16_unorm document depth".into());
    }
    let targets = FAMILIES
        .iter()
        .find(|(name, _)| *name == operation)
        .map(|(_, targets)| *targets)
        .ok_or_else(|| format!("unsupported high-depth operation: {operation}"))?;
    if !targets.contains(&target) {
        return Err(format!(
            "high-depth {operation} does not support target {target}"
        ));
    }
    Ok(())
}

pub fn capability(operation: &str, target: &str, depth: u32) -> Value {
    let decision = require(operation, target, depth);
    let storage = if depth == 32 {
        crate::float_document::STORAGE_FLOAT
    } else {
        "rgba16_unorm"
    };
    json!({"schema_version":1,"storage":storage,"evaluation":crate::precision_document::EVALUATION,
        "depth":depth,"operation":operation,"target":target,"supported":decision.is_ok(),
        "reason":decision.err(),"scope":"qualified core operation family; strict parameter, record-kind and resource validation still applies",
        "conversion":"flattened appearance only; editable structure is discarded",
        "coverage":"persistent masks/channels u16; byte tool coverage requires explicit multiplication by 257"})
}
