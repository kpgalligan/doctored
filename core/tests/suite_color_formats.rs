//! The colour, metadata and file-format test files as one binary: `color_*`, `metadata_*`, `icc_*`, `export_*`, `conversion_*`, `native_*`, `float_*`, `cmyk_*`, and the import and proof files that belong with them.
#![allow(clippy::duplicate_mod)]

#[path = "common/bw_tint8_reference.rs"]
mod bw_tint8_reference;
#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_geometry_fixture.rs"]
mod cmyk_geometry_fixture;
#[path = "common/cmyk_jpeg_builder.rs"]
mod cmyk_jpeg_builder;
#[path = "common/cmyk_paste_fixture.rs"]
mod cmyk_paste_fixture;
#[path = "common/cmyk_psd_fixture.rs"]
mod cmyk_psd_fixture;
#[path = "common/cmyk_structure_fixture.rs"]
mod cmyk_structure_fixture;
#[path = "common/cmyk_style_fixture.rs"]
mod cmyk_style_fixture;
#[path = "common/cmyk_tiff_builder.rs"]
mod cmyk_tiff_builder;
mod common;
#[path = "common/exr_builder.rs"]
mod exr_builder;
#[path = "common/extended_box_reference.rs"]
mod extended_box_reference;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/gray_fixture.rs"]
mod gray_fixture;
#[path = "common/gray_ops.rs"]
mod gray_ops;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/icc_spelling.rs"]
mod icc_spelling;
#[path = "common/indexed_fixture.rs"]
mod indexed_fixture;
#[path = "common/inverse_mapping.rs"]
mod inverse_mapping;
#[path = "common/lab_blend_oracle.rs"]
mod lab_blend_oracle;
#[path = "common/lab_fixture.rs"]
mod lab_fixture;
#[path = "common/lab_psd_fixture.rs"]
mod lab_psd_fixture;
mod live_filter_support;
#[path = "common/native_blend_oracle.rs"]
mod native_blend_oracle;
#[path = "common/native_scalar.rs"]
mod native_scalar;
#[path = "common/precision_select.rs"]
mod precision_select;
#[path = "common/psb_fixture.rs"]
mod psb_fixture;
#[path = "common/psd16_fixture.rs"]
mod psd16_fixture;
#[path = "common/psd_descriptor_fixture.rs"]
mod psd_descriptor_fixture;
#[path = "common/stack_oracle.rs"]
mod stack_oracle;
#[path = "common/vibrance_reference.rs"]
mod vibrance_reference;
#[allow(dead_code)]
#[path = "common/warp_fixture.rs"]
mod warp_fixture;
#[path = "common/warp_oracle.rs"]
mod warp_oracle;

#[path = "bigtiff_import.rs"]
mod bigtiff_import;
#[path = "bitmap_edits.rs"]
mod bitmap_edits;
#[path = "bitmap_files.rs"]
mod bitmap_files;
#[path = "bitmap_methods.rs"]
mod bitmap_methods;
#[path = "bitmap_mode.rs"]
mod bitmap_mode;
#[path = "bitmap_native.rs"]
mod bitmap_native;
#[path = "bitmap_photoshop_take.rs"]
mod bitmap_photoshop_take;
#[path = "bitmap_psd.rs"]
mod bitmap_psd;
#[path = "bitmap_resampled.rs"]
mod bitmap_resampled;
#[path = "bitmap_tools.rs"]
mod bitmap_tools;
#[path = "black_and_white_tint.rs"]
mod black_and_white_tint;
#[path = "builtin_wide_profiles.rs"]
mod builtin_wide_profiles;
#[path = "cmyk_adjust.rs"]
mod cmyk_adjust;
#[path = "cmyk_adjust_ink.rs"]
mod cmyk_adjust_ink;
#[path = "cmyk_adjustment_styles.rs"]
mod cmyk_adjustment_styles;
#[path = "cmyk_align_stack.rs"]
mod cmyk_align_stack;
#[path = "cmyk_artwork.rs"]
mod cmyk_artwork;
#[path = "cmyk_blend.rs"]
mod cmyk_blend;
#[path = "cmyk_blend_if.rs"]
mod cmyk_blend_if;
#[path = "cmyk_channels.rs"]
mod cmyk_channels;
#[path = "cmyk_clipboard.rs"]
mod cmyk_clipboard;
#[path = "cmyk_colour_profiles.rs"]
mod cmyk_colour_profiles;
#[path = "cmyk_document.rs"]
mod cmyk_document;
#[path = "cmyk_edit.rs"]
mod cmyk_edit;
#[path = "cmyk_equalize.rs"]
mod cmyk_equalize;
#[path = "cmyk_export.rs"]
mod cmyk_export;
#[path = "cmyk_files.rs"]
mod cmyk_files;
#[path = "cmyk_filter.rs"]
mod cmyk_filter;
#[path = "cmyk_generative_fill.rs"]
mod cmyk_generative_fill;
#[path = "cmyk_geometry.rs"]
mod cmyk_geometry;
#[path = "cmyk_geometry_structure.rs"]
mod cmyk_geometry_structure;
#[path = "cmyk_hue_saturation.rs"]
mod cmyk_hue_saturation;
#[path = "cmyk_knockout.rs"]
mod cmyk_knockout;
#[path = "cmyk_merge.rs"]
mod cmyk_merge;
#[path = "cmyk_merge_selected.rs"]
mod cmyk_merge_selected;
#[path = "cmyk_mode_photoshop.rs"]
mod cmyk_mode_photoshop;
#[path = "cmyk_paint_modes.rs"]
mod cmyk_paint_modes;
#[path = "cmyk_pass_through_styles.rs"]
mod cmyk_pass_through_styles;
#[path = "cmyk_pattern_overlay.rs"]
mod cmyk_pattern_overlay;
#[path = "cmyk_plate_target.rs"]
mod cmyk_plate_target;
#[path = "cmyk_proof.rs"]
mod cmyk_proof;
#[path = "cmyk_psd_export.rs"]
mod cmyk_psd_export;
#[path = "cmyk_psd_import.rs"]
mod cmyk_psd_import;
#[path = "cmyk_psd_photoshop.rs"]
mod cmyk_psd_photoshop;
#[path = "cmyk_psd_styles.rs"]
mod cmyk_psd_styles;
#[path = "cmyk_retouch.rs"]
mod cmyk_retouch;
#[path = "cmyk_retouch_brush.rs"]
mod cmyk_retouch_brush;
#[path = "cmyk_retouch_options.rs"]
mod cmyk_retouch_options;
#[path = "cmyk_select.rs"]
mod cmyk_select;
#[path = "cmyk_select_kinds.rs"]
mod cmyk_select_kinds;
#[path = "cmyk_selected_pixels.rs"]
mod cmyk_selected_pixels;
#[path = "cmyk_selected_plate.rs"]
mod cmyk_selected_plate;
#[path = "cmyk_spot_heal_types.rs"]
mod cmyk_spot_heal_types;
#[path = "cmyk_stack_mode.rs"]
mod cmyk_stack_mode;
#[path = "cmyk_stroke_mask.rs"]
mod cmyk_stroke_mask;
#[path = "cmyk_structure.rs"]
mod cmyk_structure;
#[path = "cmyk_styles.rs"]
mod cmyk_styles;
#[path = "cmyk_styles_stack.rs"]
mod cmyk_styles_stack;
#[path = "cmyk_styles_stacked.rs"]
mod cmyk_styles_stacked;
#[path = "cmyk_tiff_samples.rs"]
mod cmyk_tiff_samples;
#[path = "cmyk_tone_tools.rs"]
mod cmyk_tone_tools;
#[path = "cmyk_warp.rs"]
mod cmyk_warp;
#[path = "color_format.rs"]
mod color_format;
#[path = "color_intent_tests.rs"]
mod color_intent_tests;
#[path = "color_samplers.rs"]
mod color_samplers;
#[path = "color_tests.rs"]
mod color_tests;
#[path = "conversion_bake_plan.rs"]
mod conversion_bake_plan;
#[path = "conversion_lut_destination.rs"]
mod conversion_lut_destination;
#[path = "conversion_policy_tests.rs"]
mod conversion_policy_tests;
#[path = "conversion_preflight_job.rs"]
mod conversion_preflight_job;
#[path = "convert_rasterize_sources.rs"]
mod convert_rasterize_sources;
#[path = "duotone_close.rs"]
mod duotone_close;
#[path = "duotone_convert.rs"]
mod duotone_convert;
#[path = "duotone_document.rs"]
mod duotone_document;
#[path = "duotone_files.rs"]
mod duotone_files;
#[path = "duotone_native.rs"]
mod duotone_native;
#[path = "duotone_photoshop_take.rs"]
mod duotone_photoshop_take;
#[path = "duotone_surfaces.rs"]
mod duotone_surfaces;
#[path = "encode_refusals.rs"]
mod encode_refusals;
#[path = "encode_refusals_falling.rs"]
mod encode_refusals_falling;
#[path = "export_hdr_photo.rs"]
mod export_hdr_photo;
#[path = "export_host_formats.rs"]
mod export_host_formats;
#[path = "export_modern_formats.rs"]
mod export_modern_formats;
#[path = "export_pipeline.rs"]
mod export_pipeline;
#[path = "export_production.rs"]
mod export_production;
#[path = "export_side_limits.rs"]
mod export_side_limits;
#[path = "export_smart_sharpen.rs"]
mod export_smart_sharpen;
#[path = "float_adjust_apply.rs"]
mod float_adjust_apply;
#[path = "float_adjustments.rs"]
mod float_adjustments;
#[path = "float_artwork.rs"]
mod float_artwork;
#[path = "float_blend_modes.rs"]
mod float_blend_modes;
#[path = "float_brush_tools.rs"]
mod float_brush_tools;
#[path = "float_display_adjustments.rs"]
mod float_display_adjustments;
#[path = "float_display_extended.rs"]
mod float_display_extended;
#[path = "float_document.rs"]
mod float_document;
#[path = "float_exact_geometry.rs"]
mod float_exact_geometry;
#[path = "float_exr.rs"]
mod float_exr;
#[path = "float_fill_stroke.rs"]
mod float_fill_stroke;
#[path = "float_filtered_geometry.rs"]
mod float_filtered_geometry;
#[path = "float_filters.rs"]
mod float_filters;
#[path = "float_filters_develop.rs"]
mod float_filters_develop;
#[path = "float_filters_display.rs"]
mod float_filters_display;
#[path = "float_filters_geometry.rs"]
mod float_filters_geometry;
#[path = "float_filters_highlight.rs"]
mod float_filters_highlight;
#[path = "float_filters_smart_sharpen.rs"]
mod float_filters_smart_sharpen;
#[path = "float_hdr_local_adaptation.rs"]
mod float_hdr_local_adaptation;
#[path = "float_hdr_toning.rs"]
mod float_hdr_toning;
#[path = "float_hdr_toning_photoshop.rs"]
mod float_hdr_toning_photoshop;
#[path = "float_host_intake.rs"]
mod float_host_intake;
#[path = "float_interchange.rs"]
mod float_interchange;
#[path = "float_layer_edits.rs"]
mod float_layer_edits;
#[path = "float_layer_export.rs"]
mod float_layer_export;
#[path = "float_layer_styles.rs"]
mod float_layer_styles;
#[path = "float_lookup_tint_map.rs"]
mod float_lookup_tint_map;
#[path = "float_master_low_depth.rs"]
mod float_master_low_depth;
#[path = "float_merge_hdr.rs"]
mod float_merge_hdr;
#[path = "float_native.rs"]
mod float_native;
#[path = "float_paint_tone.rs"]
mod float_paint_tone;
#[path = "float_pass_through.rs"]
mod float_pass_through;
#[path = "float_pattern_overlay.rs"]
mod float_pattern_overlay;
#[path = "float_placed_image.rs"]
mod float_placed_image;
#[path = "float_placed_master.rs"]
mod float_placed_master;
#[path = "float_plane_fill.rs"]
mod float_plane_fill;
#[path = "float_plane_filters.rs"]
mod float_plane_filters;
#[path = "float_profile_edit.rs"]
mod float_profile_edit;
#[path = "float_promote.rs"]
mod float_promote;
#[path = "float_psd_export.rs"]
mod float_psd_export;
#[path = "float_radiance.rs"]
mod float_radiance;
#[path = "float_raw_source.rs"]
mod float_raw_source;
#[path = "float_retouch.rs"]
mod float_retouch;
#[path = "float_retouch_brush.rs"]
mod float_retouch_brush;
#[path = "float_scoped_statistics.rs"]
mod float_scoped_statistics;
#[path = "float_selected_pixels.rs"]
mod float_selected_pixels;
#[path = "float_smart_selection.rs"]
mod float_smart_selection;
#[path = "float_smudge_plan.rs"]
mod float_smudge_plan;
#[path = "float_source_selection.rs"]
mod float_source_selection;
#[path = "float_view.rs"]
mod float_view;
#[path = "float_warp.rs"]
mod float_warp;
#[path = "gradient_map_method_native.rs"]
mod gradient_map_method_native;
#[path = "gradient_midpoint_native.rs"]
mod gradient_midpoint_native;
#[path = "gradient_smoothness_align_native.rs"]
mod gradient_smoothness_align_native;
#[path = "gray16_import_tests.rs"]
mod gray16_import_tests;
#[path = "gray32_files.rs"]
mod gray32_files;
#[path = "gray_adjust.rs"]
mod gray_adjust;
#[path = "gray_analysis.rs"]
mod gray_analysis;
#[path = "gray_artwork.rs"]
mod gray_artwork;
#[path = "gray_channels.rs"]
mod gray_channels;
#[path = "gray_clipboard.rs"]
mod gray_clipboard;
#[path = "gray_close.rs"]
mod gray_close;
#[path = "gray_colour.rs"]
mod gray_colour;
#[path = "gray_comps.rs"]
mod gray_comps;
#[path = "gray_document.rs"]
mod gray_document;
#[path = "gray_export.rs"]
mod gray_export;
#[path = "gray_files.rs"]
mod gray_files;
#[path = "gray_filter.rs"]
mod gray_filter;
#[path = "gray_geometry.rs"]
mod gray_geometry;
#[path = "gray_neural.rs"]
mod gray_neural;
#[path = "gray_paint.rs"]
mod gray_paint;
#[path = "gray_proof_cmyk.rs"]
mod gray_proof_cmyk;
#[path = "gray_psd.rs"]
mod gray_psd;
#[path = "gray_psd_export.rs"]
mod gray_psd_export;
#[path = "gray_psd_photoshop.rs"]
mod gray_psd_photoshop;
#[path = "gray_retouch.rs"]
mod gray_retouch;
#[path = "gray_structure.rs"]
mod gray_structure;
#[path = "gray_style.rs"]
mod gray_style;
#[path = "gray_tools.rs"]
mod gray_tools;
#[path = "gray_working_selection.rs"]
mod gray_working_selection;
#[path = "hdr_transfer.rs"]
mod hdr_transfer;
#[path = "heif_container.rs"]
mod heif_container;
#[path = "icc_describe_fields.rs"]
mod icc_describe_fields;
#[path = "icc_lookup_rebake.rs"]
mod icc_lookup_rebake;
#[path = "icc_lookup_tests.rs"]
mod icc_lookup_tests;
#[path = "icc_lut_tests.rs"]
mod icc_lut_tests;
#[path = "indexed_background.rs"]
mod indexed_background;
#[path = "indexed_blend_modes.rs"]
mod indexed_blend_modes;
#[path = "indexed_color_table.rs"]
mod indexed_color_table;
#[path = "indexed_convert.rs"]
mod indexed_convert;
#[path = "indexed_custom.rs"]
mod indexed_custom;
#[path = "indexed_dither.rs"]
mod indexed_dither;
#[path = "indexed_edits.rs"]
mod indexed_edits;
#[path = "indexed_export.rs"]
mod indexed_export;
#[path = "indexed_layer_ops.rs"]
mod indexed_layer_ops;
#[path = "indexed_native.rs"]
mod indexed_native;
#[path = "indexed_open.rs"]
mod indexed_open;
#[path = "indexed_paint.rs"]
mod indexed_paint;
#[path = "indexed_palette.rs"]
mod indexed_palette;
#[path = "indexed_palette_take.rs"]
mod indexed_palette_take;
#[path = "indexed_paste.rs"]
mod indexed_paste;
#[path = "indexed_photoshop_take.rs"]
mod indexed_photoshop_take;
#[path = "indexed_profile.rs"]
mod indexed_profile;
#[path = "indexed_psd.rs"]
mod indexed_psd;
#[path = "indexed_psd_photoshop.rs"]
mod indexed_psd_photoshop;
#[path = "indexed_selections.rs"]
mod indexed_selections;
#[path = "indexed_tiff.rs"]
mod indexed_tiff;
#[path = "indexed_tools.rs"]
mod indexed_tools;
#[path = "indexed_transforms.rs"]
mod indexed_transforms;
#[path = "lab_adjust.rs"]
mod lab_adjust;
#[path = "lab_analysis.rs"]
mod lab_analysis;
#[path = "lab_artwork.rs"]
mod lab_artwork;
#[path = "lab_blend.rs"]
mod lab_blend;
#[path = "lab_blend_if.rs"]
mod lab_blend_if;
#[path = "lab_channel_filter.rs"]
mod lab_channel_filter;
#[path = "lab_clipboard.rs"]
mod lab_clipboard;
#[path = "lab_close.rs"]
mod lab_close;
#[path = "lab_close_photoshop.rs"]
mod lab_close_photoshop;
#[path = "lab_cmyk_depth.rs"]
mod lab_cmyk_depth;
#[path = "lab_cmyk_photoshop.rs"]
mod lab_cmyk_photoshop;
#[path = "lab_color_adjust.rs"]
mod lab_color_adjust;
#[path = "lab_color_select.rs"]
mod lab_color_select;
#[path = "lab_comps.rs"]
mod lab_comps;
#[path = "lab_convert.rs"]
mod lab_convert;
#[path = "lab_document.rs"]
mod lab_document;
#[path = "lab_effects.rs"]
mod lab_effects;
#[path = "lab_filter.rs"]
mod lab_filter;
#[path = "lab_geometry.rs"]
mod lab_geometry;
#[path = "lab_gray.rs"]
mod lab_gray;
#[path = "lab_legacy_ops.rs"]
mod lab_legacy_ops;
#[path = "lab_model_select.rs"]
mod lab_model_select;
#[path = "lab_native.rs"]
mod lab_native;
#[path = "lab_neural.rs"]
mod lab_neural;
#[path = "lab_paint.rs"]
mod lab_paint;
#[path = "lab_pattern_stroke.rs"]
mod lab_pattern_stroke;
#[path = "lab_picker_tests.rs"]
mod lab_picker_tests;
#[path = "lab_proof_profile.rs"]
mod lab_proof_profile;
#[path = "lab_psd.rs"]
mod lab_psd;
#[path = "lab_psd_export.rs"]
mod lab_psd_export;
#[path = "lab_psd_photoshop.rs"]
mod lab_psd_photoshop;
#[path = "lab_readouts.rs"]
mod lab_readouts;
#[path = "lab_retouch.rs"]
mod lab_retouch;
#[path = "lab_save.rs"]
mod lab_save;
#[path = "lab_selection.rs"]
mod lab_selection;
#[path = "lab_stack_tools.rs"]
mod lab_stack_tools;
#[path = "lab_structure.rs"]
mod lab_structure;
#[path = "lab_targets.rs"]
mod lab_targets;
#[path = "lab_tiff_open.rs"]
mod lab_tiff_open;
#[path = "lab_tools.rs"]
mod lab_tools;
#[path = "lab_wand.rs"]
mod lab_wand;
#[path = "lab_wand_photoshop.rs"]
mod lab_wand_photoshop;
#[path = "lab_wide_gamut.rs"]
mod lab_wide_gamut;
#[path = "large_description_native.rs"]
mod large_description_native;
#[path = "lut_authored_colors.rs"]
mod lut_authored_colors;
#[path = "metadata_extended_xmp_read.rs"]
mod metadata_extended_xmp_read;
#[path = "metadata_extended_xmp_write.rs"]
mod metadata_extended_xmp_write;
#[path = "metadata_file_info.rs"]
mod metadata_file_info;
#[path = "metadata_import.rs"]
mod metadata_import;
#[path = "metadata_packet_framing.rs"]
mod metadata_packet_framing;
#[path = "metadata_tests.rs"]
mod metadata_tests;
#[path = "metadata_tiff_tests.rs"]
mod metadata_tiff_tests;
#[path = "multichannel_psd.rs"]
mod multichannel_psd;
#[path = "native_banded_reopen.rs"]
mod native_banded_reopen;
#[path = "native_capacity.rs"]
mod native_capacity;
#[path = "native_png_bound.rs"]
mod native_png_bound;
#[path = "native_save_bytes.rs"]
mod native_save_bytes;
#[path = "open_profile_tests.rs"]
mod open_profile_tests;
#[path = "pattern_fill_cmyk.rs"]
mod pattern_fill_cmyk;
#[path = "precision_blend_if_multichannel.rs"]
mod precision_blend_if_multichannel;
#[path = "precision_contour.rs"]
mod precision_contour;
#[path = "precision_glow_gradient.rs"]
mod precision_glow_gradient;
#[path = "precision_noise.rs"]
mod precision_noise;
#[path = "precision_pattern_angle.rs"]
mod precision_pattern_angle;
#[path = "precision_pattern_overlay.rs"]
mod precision_pattern_overlay;
#[path = "precision_preservation_tests.rs"]
mod precision_preservation_tests;
#[path = "precision_stroke_fill.rs"]
mod precision_stroke_fill;
#[path = "precision_stroke_pattern.rs"]
mod precision_stroke_pattern;
#[path = "precision_texture.rs"]
mod precision_texture;
#[path = "raw_place.rs"]
mod raw_place;
#[path = "soft_proof_tests.rs"]
mod soft_proof_tests;
#[path = "spot_channel_ffi.rs"]
mod spot_channel_ffi;
#[path = "spot_channels.rs"]
mod spot_channels;
#[path = "spot_merge.rs"]
mod spot_merge;
#[path = "spot_psd.rs"]
mod spot_psd;
#[path = "spot_psd_photoshop.rs"]
mod spot_psd_photoshop;
#[path = "spot_tiff.rs"]
mod spot_tiff;
#[path = "spot_tiff_layers.rs"]
mod spot_tiff_layers;
#[path = "spot_tiff_photoshop.rs"]
mod spot_tiff_photoshop;
#[path = "spot_typed_export.rs"]
mod spot_typed_export;
#[path = "tiff_float_tests.rs"]
mod tiff_float_tests;
#[path = "tiff_layers.rs"]
mod tiff_layers;
