//! The typed-document (`precision_*`) test files as one binary.
#![allow(clippy::duplicate_mod)]

#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_structure_fixture.rs"]
mod cmyk_structure_fixture;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/group_roots.rs"]
mod group_roots;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/identity.rs"]
mod identity;
#[path = "common/inverse_mapping.rs"]
mod inverse_mapping;
#[path = "common/native_blend_oracle.rs"]
mod native_blend_oracle;
#[path = "common/native_scalar.rs"]
mod native_scalar;
#[path = "common/perspective_oracle.rs"]
mod perspective_oracle;
#[path = "common/precision_native_fixture.rs"]
mod precision_native_fixture;
#[path = "common/psd16_fixture.rs"]
mod psd16_fixture;
#[path = "common/source_refusal_fixture.rs"]
mod source_refusal_fixture;
#[path = "common/stack_oracle.rs"]
mod stack_oracle;

#[path = "gradient_tool_midpoints.rs"]
mod gradient_tool_midpoints;
#[path = "gradient_tool_smoothness.rs"]
mod gradient_tool_smoothness;
#[path = "group_projection_rule.rs"]
mod group_projection_rule;
#[path = "native_stream_reader.rs"]
mod native_stream_reader;
#[path = "precision_add_layer.rs"]
mod precision_add_layer;
#[path = "precision_adjustment_blending.rs"]
mod precision_adjustment_blending;
#[path = "precision_adjustment_math.rs"]
mod precision_adjustment_math;
#[path = "precision_adjustment_styles.rs"]
mod precision_adjustment_styles;
#[path = "precision_adjustments.rs"]
mod precision_adjustments;
#[path = "precision_align.rs"]
mod precision_align;
#[path = "precision_blank.rs"]
mod precision_blank;
#[path = "precision_blend_if.rs"]
mod precision_blend_if;
#[path = "precision_blend_mask_wrappers.rs"]
mod precision_blend_mask_wrappers;
#[path = "precision_brush_tools.rs"]
mod precision_brush_tools;
#[path = "precision_canvas_rotation.rs"]
mod precision_canvas_rotation;
#[path = "precision_canvas_size.rs"]
mod precision_canvas_size;
#[path = "precision_channel_math.rs"]
mod precision_channel_math;
#[path = "precision_channels.rs"]
mod precision_channels;
#[path = "precision_clipboard.rs"]
mod precision_clipboard;
#[path = "precision_clipboard_boundaries.rs"]
mod precision_clipboard_boundaries;
#[path = "precision_clipboard_editable.rs"]
mod precision_clipboard_editable;
#[path = "precision_clipboard_group.rs"]
mod precision_clipboard_group;
#[path = "precision_clipboard_into.rs"]
mod precision_clipboard_into;
#[path = "precision_clipboard_promote.rs"]
mod precision_clipboard_promote;
#[path = "precision_clipboard_trim.rs"]
mod precision_clipboard_trim;
#[path = "precision_clipping.rs"]
mod precision_clipping;
#[path = "precision_codec.rs"]
mod precision_codec;
#[path = "precision_color.rs"]
mod precision_color;
#[path = "precision_compositor_parity.rs"]
mod precision_compositor_parity;
#[path = "precision_conversion.rs"]
mod precision_conversion;
#[path = "precision_conversion_channels.rs"]
mod precision_conversion_channels;
#[path = "precision_conversion_narrow.rs"]
mod precision_conversion_narrow;
#[path = "precision_conversion_structure.rs"]
mod precision_conversion_structure;
#[path = "precision_cutout.rs"]
mod precision_cutout;
#[path = "precision_document.rs"]
mod precision_document;
#[path = "precision_duplicate_parity.rs"]
mod precision_duplicate_parity;
#[path = "precision_edge_refine.rs"]
mod precision_edge_refine;
#[path = "precision_ffi.rs"]
mod precision_ffi;
#[path = "precision_fill_stroke.rs"]
mod precision_fill_stroke;
#[path = "precision_four_blending.rs"]
mod precision_four_blending;
#[path = "precision_gaussian.rs"]
mod precision_gaussian;
#[path = "precision_geometry.rs"]
mod precision_geometry;
#[path = "precision_global_light.rs"]
mod precision_global_light;
#[path = "precision_gradient_stops.rs"]
mod precision_gradient_stops;
#[path = "precision_group_open.rs"]
mod precision_group_open;
#[path = "precision_groups.rs"]
mod precision_groups;
#[path = "precision_guides.rs"]
mod precision_guides;
#[path = "precision_healing.rs"]
mod precision_healing;
#[path = "precision_hsl_blending.rs"]
mod precision_hsl_blending;
#[path = "precision_icc_math.rs"]
mod precision_icc_math;
#[path = "precision_image_accessor.rs"]
mod precision_image_accessor;
#[path = "precision_inpaint.rs"]
mod precision_inpaint;
#[path = "precision_insert.rs"]
mod precision_insert;
#[path = "precision_inspection.rs"]
mod precision_inspection;
#[path = "precision_intake_name.rs"]
mod precision_intake_name;
#[path = "precision_knockout.rs"]
mod precision_knockout;
#[path = "precision_knockout_format.rs"]
mod precision_knockout_format;
#[path = "precision_layer_blending.rs"]
mod precision_layer_blending;
#[path = "precision_layer_canvas_image.rs"]
mod precision_layer_canvas_image;
#[path = "precision_layer_hit.rs"]
mod precision_layer_hit;
#[path = "precision_layer_styles.rs"]
mod precision_layer_styles;
#[path = "precision_layer_via.rs"]
mod precision_layer_via;
#[path = "precision_locks.rs"]
mod precision_locks;
#[path = "precision_mask_invert.rs"]
mod precision_mask_invert;
#[path = "precision_masks.rs"]
mod precision_masks;
#[path = "precision_merge.rs"]
mod precision_merge;
#[path = "precision_merge_extent.rs"]
mod precision_merge_extent;
#[path = "precision_merge_selected.rs"]
mod precision_merge_selected;
#[path = "precision_metadata_bridge.rs"]
mod precision_metadata_bridge;
#[path = "precision_paint.rs"]
mod precision_paint;
#[path = "precision_pass_through_styles.rs"]
mod precision_pass_through_styles;
#[path = "precision_perspective_crop.rs"]
mod precision_perspective_crop;
#[path = "precision_projection_export.rs"]
mod precision_projection_export;
#[path = "precision_prototype.rs"]
mod precision_prototype;
#[path = "precision_redeye.rs"]
mod precision_redeye;
#[path = "precision_remaining_separable.rs"]
mod precision_remaining_separable;
#[path = "precision_resolution.rs"]
mod precision_resolution;
#[path = "precision_retouch.rs"]
mod precision_retouch;
#[path = "precision_retouch_brush.rs"]
mod precision_retouch_brush;
#[path = "precision_retouch_progress.rs"]
mod precision_retouch_progress;
#[path = "precision_scalar_selection.rs"]
mod precision_scalar_selection;
#[path = "precision_scalar_statistics.rs"]
mod precision_scalar_statistics;
#[path = "precision_selection_refine.rs"]
mod precision_selection_refine;
#[path = "precision_stack_mode.rs"]
mod precision_stack_mode;
#[path = "precision_stamp_flatten.rs"]
mod precision_stamp_flatten;
#[path = "precision_stats.rs"]
mod precision_stats;
#[path = "precision_straighten.rs"]
mod precision_straighten;
#[path = "precision_structure.rs"]
mod precision_structure;
#[path = "precision_structure_ops.rs"]
mod precision_structure_ops;
#[path = "precision_threshold_blending.rs"]
mod precision_threshold_blending;
#[path = "precision_tone_accuracy.rs"]
mod precision_tone_accuracy;
#[path = "precision_tool_blend.rs"]
mod precision_tool_blend;
#[path = "precision_vector_masks.rs"]
mod precision_vector_masks;
#[path = "precision_work_caps.rs"]
mod precision_work_caps;
