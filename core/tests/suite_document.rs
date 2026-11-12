//! The document-model test files as one binary: `layer_*`, `group_*`, `mask_*`, `knockout_*`, `guide_*`, `geometry_*`, `canvas_*`, `byte_*`, the files with no family prefix that belong with them, and the layout guard (`test_layout`).
#![allow(clippy::duplicate_mod)]

#[path = "common/auto_blend_fixture.rs"]
mod auto_blend_fixture;
#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_structure_fixture.rs"]
mod cmyk_structure_fixture;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/icc_spelling.rs"]
mod icc_spelling;
#[path = "common/identity.rs"]
mod identity;
#[path = "common/inverse_mapping.rs"]
mod inverse_mapping;
mod live_filter_support;
#[path = "common/native_blend_oracle.rs"]
mod native_blend_oracle;
#[path = "common/native_scalar.rs"]
mod native_scalar;
#[path = "common/perspective_oracle.rs"]
mod perspective_oracle;
#[path = "common/phase4_corpus.rs"]
mod phase4_corpus;
#[path = "common/psd16_fixture.rs"]
mod psd16_fixture;
#[path = "common/psd_mask_fixture.rs"]
mod psd_mask_fixture;
#[path = "common/resource_report.rs"]
mod resource_report;
#[path = "common/rgba_layer.rs"]
mod rgba_layer;
#[path = "common/source_refusal_fixture.rs"]
mod source_refusal_fixture;
#[path = "common/stack_oracle.rs"]
mod stack_oracle;

#[path = "agent_tests.rs"]
mod agent_tests;
#[path = "byte_canvas_geometry_cancel.rs"]
mod byte_canvas_geometry_cancel;
#[path = "byte_filter_op_plan.rs"]
mod byte_filter_op_plan;
#[path = "byte_gaussian_blur_op.rs"]
mod byte_gaussian_blur_op;
#[path = "byte_geometry_cancel.rs"]
mod byte_geometry_cancel;
#[path = "byte_merge_extent.rs"]
mod byte_merge_extent;
#[path = "canvas_geometry_admission.rs"]
mod canvas_geometry_admission;
#[path = "canvas_rotation.rs"]
mod canvas_rotation;
#[path = "channel_tests.rs"]
mod channel_tests;
#[path = "clip_display.rs"]
mod clip_display;
#[path = "clip_unit_frozen.rs"]
mod clip_unit_frozen;
#[path = "comps_persisted_identity.rs"]
mod comps_persisted_identity;
#[path = "crop_extend.rs"]
mod crop_extend;
#[path = "crop_extend_photoshop.rs"]
mod crop_extend_photoshop;
#[path = "crop_fill.rs"]
mod crop_fill;
#[path = "delete_cropped_pixels.rs"]
mod delete_cropped_pixels;
#[path = "duplicate_to_new_document.rs"]
mod duplicate_to_new_document;
#[path = "feature_register.rs"]
mod feature_register;
#[path = "generative_expand.rs"]
mod generative_expand;
#[path = "generative_fill.rs"]
mod generative_fill;
#[path = "generative_variations.rs"]
mod generative_variations;
#[path = "geometry_align_distortion.rs"]
mod geometry_align_distortion;
#[path = "geometry_align_planar.rs"]
mod geometry_align_planar;
#[path = "geometry_align_projection.rs"]
mod geometry_align_projection;
#[path = "geometry_align_vignette.rs"]
mod geometry_align_vignette;
#[path = "geometry_align_warp.rs"]
mod geometry_align_warp;
#[path = "geometry_noops.rs"]
mod geometry_noops;
#[path = "geometry_sets.rs"]
mod geometry_sets;
#[path = "graph_cut_min_cut.rs"]
mod graph_cut_min_cut;
#[path = "group_format.rs"]
mod group_format;
#[path = "group_tests.rs"]
mod group_tests;
#[path = "guide_format.rs"]
mod guide_format;
#[path = "guide_tests.rs"]
mod guide_tests;
#[path = "integration.rs"]
mod integration;
#[path = "knockout_composite.rs"]
mod knockout_composite;
#[path = "knockout_format.rs"]
mod knockout_format;
#[path = "knockout_merge.rs"]
mod knockout_merge;
#[path = "landmark_region.rs"]
mod landmark_region;
#[path = "layer_auto_blend.rs"]
mod layer_auto_blend;
#[path = "layer_auto_blend_panorama.rs"]
mod layer_auto_blend_panorama;
#[path = "layer_blend_edges.rs"]
mod layer_blend_edges;
#[path = "layer_byte_opacity.rs"]
mod layer_byte_opacity;
#[path = "layer_clipboard.rs"]
mod layer_clipboard;
#[path = "layer_clipboard_chrome.rs"]
mod layer_clipboard_chrome;
#[path = "layer_clipboard_float.rs"]
mod layer_clipboard_float;
#[path = "layer_clipboard_relabel.rs"]
mod layer_clipboard_relabel;
#[path = "layer_comps.rs"]
mod layer_comps;
#[path = "layer_comps_geometry.rs"]
mod layer_comps_geometry;
#[path = "layer_comps_shift.rs"]
mod layer_comps_shift;
#[path = "layer_comps_unrecorded.rs"]
mod layer_comps_unrecorded;
#[path = "layer_fill_special.rs"]
mod layer_fill_special;
#[path = "layer_growth_comps.rs"]
mod layer_growth_comps;
#[path = "layer_identity.rs"]
mod layer_identity;
#[path = "layer_identity_native.rs"]
mod layer_identity_native;
#[path = "layer_insert.rs"]
mod layer_insert;
#[path = "layer_label.rs"]
mod layer_label;
#[path = "layer_offset_links.rs"]
mod layer_offset_links;
#[path = "layer_storage_identity.rs"]
mod layer_storage_identity;
#[path = "layer_thumbnail.rs"]
mod layer_thumbnail;
#[path = "lock_tests.rs"]
mod lock_tests;
#[path = "mask_invert_whole.rs"]
mod mask_invert_whole;
#[path = "mask_outside_coverage.rs"]
mod mask_outside_coverage;
#[path = "mask_set.rs"]
mod mask_set;
#[path = "mask_tests.rs"]
mod mask_tests;
#[path = "merge_comps.rs"]
mod merge_comps;
#[path = "model_mask.rs"]
mod model_mask;
#[path = "native_magic.rs"]
mod native_magic;
#[path = "neural_colorize.rs"]
mod neural_colorize;
#[path = "neural_filters.rs"]
mod neural_filters;
#[path = "operation_scope.rs"]
mod operation_scope;
#[path = "path_format.rs"]
mod path_format;
#[path = "path_geometry.rs"]
mod path_geometry;
#[path = "path_shape.rs"]
mod path_shape;
#[path = "path_trace.rs"]
mod path_trace;
#[path = "perspective_crop_tests.rs"]
mod perspective_crop_tests;
#[path = "perspective_tests.rs"]
mod perspective_tests;
#[path = "placement_insert.rs"]
mod placement_insert;
#[path = "preservation_tests.rs"]
mod preservation_tests;
#[path = "preserve_transparency.rs"]
mod preserve_transparency;
#[path = "quick_mask.rs"]
mod quick_mask;
#[path = "remove_model.rs"]
mod remove_model;
#[path = "remove_model_depths.rs"]
mod remove_model_depths;
#[path = "render_failure_tests.rs"]
mod render_failure_tests;
#[path = "save_leftovers.rs"]
mod save_leftovers;
#[path = "save_open_cancel.rs"]
mod save_open_cancel;
#[path = "sky_decontaminate.rs"]
mod sky_decontaminate;
#[path = "sky_replacement.rs"]
mod sky_replacement;
#[path = "sky_replacement_group.rs"]
mod sky_replacement_group;
#[path = "stack_mode_statistics.rs"]
mod stack_mode_statistics;
#[path = "stack_mode_tests.rs"]
mod stack_mode_tests;
#[path = "stats_tests.rs"]
mod stats_tests;
#[path = "structure_tests.rs"]
mod structure_tests;
#[path = "test_layout.rs"]
mod test_layout;
#[path = "tile_store.rs"]
mod tile_store;
#[path = "trim_bounds_tests.rs"]
mod trim_bounds_tests;
#[path = "uniform_plane_turn.rs"]
mod uniform_plane_turn;
#[path = "vector_masks.rs"]
mod vector_masks;
