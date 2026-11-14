//! The PSD import and export (`psd_*`) test files as one binary.
#![allow(clippy::duplicate_mod)]

#[path = "common/asl_layout.rs"]
mod asl_layout;
#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_jpeg_builder.rs"]
mod cmyk_jpeg_builder;
#[path = "common/cmyk_psd_fixture.rs"]
mod cmyk_psd_fixture;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/identity.rs"]
mod identity;
#[path = "common/psb_fixture.rs"]
mod psb_fixture;
#[path = "common/psd16_fixture.rs"]
mod psd16_fixture;
#[path = "common/psd_descriptor_fixture.rs"]
mod psd_descriptor_fixture;
#[path = "common/psd_mask_fixture.rs"]
mod psd_mask_fixture;

#[path = "psd_adjustment_bake.rs"]
mod psd_adjustment_bake;
#[path = "psd_adjustment_descriptors.rs"]
mod psd_adjustment_descriptors;
#[path = "psd_adjustment_styles.rs"]
mod psd_adjustment_styles;
#[path = "psd_adjustments.rs"]
mod psd_adjustments;
#[path = "psd_asl_export.rs"]
mod psd_asl_export;
#[path = "psd_asl_import.rs"]
mod psd_asl_import;
#[path = "psd_asl_layout.rs"]
mod psd_asl_layout;
#[path = "psd_asl_open_668.rs"]
mod psd_asl_open_668;
#[path = "psd_asl_photoshop.rs"]
mod psd_asl_photoshop;
#[path = "psd_authored_exchange.rs"]
mod psd_authored_exchange;
#[path = "psd_background.rs"]
mod psd_background;
#[path = "psd_bevel_technique.rs"]
mod psd_bevel_technique;
#[path = "psd_channel_mixer.rs"]
mod psd_channel_mixer;
#[path = "psd_clipping_path.rs"]
mod psd_clipping_path;
#[path = "psd_comp_styles.rs"]
mod psd_comp_styles;
#[path = "psd_composite_transparency.rs"]
mod psd_composite_transparency;
#[path = "psd_comps.rs"]
mod psd_comps;
#[path = "psd_constructs.rs"]
mod psd_constructs;
#[path = "psd_contour.rs"]
mod psd_contour;
#[path = "psd_curves_map.rs"]
mod psd_curves_map;
#[path = "psd_depth16.rs"]
mod psd_depth16;
#[path = "psd_depth32.rs"]
mod psd_depth32;
#[path = "psd_depth32_profile.rs"]
mod psd_depth32_profile;
#[path = "psd_disabled_effects.rs"]
mod psd_disabled_effects;
#[path = "psd_export.rs"]
mod psd_export;
#[path = "psd_export_corpus.rs"]
mod psd_export_corpus;
#[path = "psd_export_document_losses.rs"]
mod psd_export_document_losses;
#[path = "psd_feature_markers.rs"]
mod psd_feature_markers;
#[path = "psd_float_fill_style.rs"]
mod psd_float_fill_style;
#[path = "psd_glow_gradient.rs"]
mod psd_glow_gradient;
#[path = "psd_gradient_fill_layer.rs"]
mod psd_gradient_fill_layer;
#[path = "psd_gradient_midpoints.rs"]
mod psd_gradient_midpoints;
#[path = "psd_gradient_smoothness_align.rs"]
mod psd_gradient_smoothness_align;
#[path = "psd_group_effects.rs"]
mod psd_group_effects;
#[path = "psd_group_import.rs"]
mod psd_group_import;
#[path = "psd_import_notice.rs"]
mod psd_import_notice;
#[path = "psd_knockout.rs"]
mod psd_knockout;
#[path = "psd_layer_effects.rs"]
mod psd_layer_effects;
#[path = "psd_layer_pixels.rs"]
mod psd_layer_pixels;
#[path = "psd_levels.rs"]
mod psd_levels;
#[path = "psd_locks.rs"]
mod psd_locks;
#[path = "psd_mode_exchange.rs"]
mod psd_mode_exchange;
#[path = "psd_noise.rs"]
mod psd_noise;
#[path = "psd_paths.rs"]
mod psd_paths;
#[path = "psd_pattern_fill_cmyk.rs"]
mod psd_pattern_fill_cmyk;
#[path = "psd_pattern_fill_layer.rs"]
mod psd_pattern_fill_layer;
#[path = "psd_pattern_overlay.rs"]
mod psd_pattern_overlay;
#[path = "psd_photoshop.rs"]
mod psd_photoshop;
#[path = "psd_photoshop_appearance.rs"]
mod psd_photoshop_appearance;
#[path = "psd_photoshop_channels.rs"]
mod psd_photoshop_channels;
#[path = "psd_photoshop_constructs.rs"]
mod psd_photoshop_constructs;
#[path = "psd_photoshop_depth16.rs"]
mod psd_photoshop_depth16;
#[path = "psd_photoshop_depth32.rs"]
mod psd_photoshop_depth32;
#[path = "psd_photoshop_float32_679.rs"]
mod psd_photoshop_float32_679;
#[path = "psd_psb.rs"]
mod psd_psb;
#[path = "psd_raster_masks.rs"]
mod psd_raster_masks;
#[path = "psd_smart_object_pixels.rs"]
mod psd_smart_object_pixels;
#[path = "psd_solid_fill_layer.rs"]
mod psd_solid_fill_layer;
#[path = "psd_stacked_effects.rs"]
mod psd_stacked_effects;
#[path = "psd_stroke_fill.rs"]
mod psd_stroke_fill;
#[path = "psd_stroke_pattern.rs"]
mod psd_stroke_pattern;
#[path = "psd_tests.rs"]
mod psd_tests;
#[path = "psd_texture.rs"]
mod psd_texture;
#[path = "psd_vector_mask_bake.rs"]
mod psd_vector_mask_bake;
#[path = "psd_wide_color.rs"]
mod psd_wide_color;
#[path = "psd_zip16.rs"]
mod psd_zip16;
#[path = "psd_zip8.rs"]
mod psd_zip8;
#[path = "tiff_layers32.rs"]
mod tiff_layers32;
#[path = "tiff_paths.rs"]
mod tiff_paths;
#[path = "tiff_paths_modes.rs"]
mod tiff_paths_modes;
