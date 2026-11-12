//! The layer-style and adjustment test files as one binary: `style`, `style_*`, `blend_if_*`, `adjust_*`, `adjustment_*`, `calibrated_*`, `evaluation_*`, `scoped_*`, and the two native adjustment files.
#![allow(clippy::duplicate_mod)]

#[path = "common/bw_tint8_reference.rs"]
mod bw_tint8_reference;
#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_geometry_fixture.rs"]
mod cmyk_geometry_fixture;
#[path = "common/cmyk_structure_fixture.rs"]
mod cmyk_structure_fixture;
#[path = "common/cmyk_style_fixture.rs"]
mod cmyk_style_fixture;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/identify_578.rs"]
mod identify_578;
#[path = "common/identity.rs"]
mod identity;
#[path = "common/linear_oracle.rs"]
mod linear_oracle;
#[path = "common/psd_descriptor_fixture.rs"]
mod psd_descriptor_fixture;
#[path = "common/vibrance_reference.rs"]
mod vibrance_reference;

#[path = "adjust_auto_endpoints.rs"]
mod adjust_auto_endpoints;
#[path = "adjust_auto_endpoints_rows.rs"]
mod adjust_auto_endpoints_rows;
#[path = "adjust_auto_endpoints_take8.rs"]
mod adjust_auto_endpoints_take8;
#[path = "adjust_bcs_modern_tests.rs"]
mod adjust_bcs_modern_tests;
#[path = "adjust_color_balance_parity.rs"]
mod adjust_color_balance_parity;
#[path = "adjust_color_tests.rs"]
mod adjust_color_tests;
#[path = "adjust_curves_map.rs"]
mod adjust_curves_map;
#[path = "adjust_equalize.rs"]
mod adjust_equalize;
#[path = "adjust_hue_saturation_parity.rs"]
mod adjust_hue_saturation_parity;
#[path = "adjust_levels.rs"]
mod adjust_levels;
#[path = "adjust_map_tests.rs"]
mod adjust_map_tests;
#[path = "adjust_mix_tests.rs"]
mod adjust_mix_tests;
#[path = "adjust_parity_578_take.rs"]
mod adjust_parity_578_take;
#[path = "adjust_photo_filter_tests.rs"]
mod adjust_photo_filter_tests;
#[path = "adjust_replace_color_tests.rs"]
mod adjust_replace_color_tests;
#[path = "adjust_tone_tests.rs"]
mod adjust_tone_tests;
#[path = "adjust_vibrance_posterize_parity.rs"]
mod adjust_vibrance_posterize_parity;
#[path = "adjustment_preset_files.rs"]
mod adjustment_preset_files;
#[path = "adjustment_repreparation.rs"]
mod adjustment_repreparation;
#[path = "adjustment_tests.rs"]
mod adjustment_tests;
#[path = "blend_if_gray_parity.rs"]
mod blend_if_gray_parity;
#[path = "blend_if_multichannel.rs"]
mod blend_if_multichannel;
#[path = "calibrated_adjustment_tests.rs"]
mod calibrated_adjustment_tests;
#[path = "calibrated_stored_fixture.rs"]
mod calibrated_stored_fixture;
#[path = "curves_native.rs"]
mod curves_native;
#[path = "evaluation_composite.rs"]
mod evaluation_composite;
#[path = "evaluation_ffi.rs"]
mod evaluation_ffi;
#[path = "evaluation_guide.rs"]
mod evaluation_guide;
#[path = "evaluation_native.rs"]
mod evaluation_native;
#[path = "evaluation_nonseparable.rs"]
mod evaluation_nonseparable;
#[path = "evaluation_retained_source.rs"]
mod evaluation_retained_source;
#[path = "evaluation_transfer.rs"]
mod evaluation_transfer;
#[path = "exposure_photoshop_parity.rs"]
mod exposure_photoshop_parity;
#[path = "gradient_fill_bracket.rs"]
mod gradient_fill_bracket;
#[path = "gradient_map_classic.rs"]
mod gradient_map_classic;
#[path = "gradient_map_identify_parity.rs"]
mod gradient_map_identify_parity;
#[path = "gradient_map_perceptual.rs"]
mod gradient_map_perceptual;
#[path = "levels_native.rs"]
mod levels_native;
#[path = "lut_tests.rs"]
mod lut_tests;
#[path = "pattern_fill_layer.rs"]
mod pattern_fill_layer;
#[path = "scoped_auto.rs"]
mod scoped_auto;
#[path = "scoped_statistics.rs"]
mod scoped_statistics;
#[path = "style.rs"]
mod style;
#[path = "style_adjustment_layer.rs"]
mod style_adjustment_layer;
#[path = "style_bevel_chisel_parity.rs"]
mod style_bevel_chisel_parity;
#[path = "style_bevel_chisel_take3_parity.rs"]
mod style_bevel_chisel_take3_parity;
#[path = "style_bevel_emboss.rs"]
mod style_bevel_emboss;
#[path = "style_bevel_emboss_take2_parity.rs"]
mod style_bevel_emboss_take2_parity;
#[path = "style_bevel_identify_parity.rs"]
mod style_bevel_identify_parity;
#[path = "style_bevel_kernels_578_take.rs"]
mod style_bevel_kernels_578_take;
#[path = "style_bevel_outside_law.rs"]
mod style_bevel_outside_law;
#[path = "style_bevel_technique.rs"]
mod style_bevel_technique;
#[path = "style_blend_if.rs"]
mod style_blend_if;
#[path = "style_color_overlay.rs"]
mod style_color_overlay;
#[path = "style_contour.rs"]
mod style_contour;
#[path = "style_effect_kernels_578_p3_take.rs"]
mod style_effect_kernels_578_p3_take;
#[path = "style_effect_kernels_578_p4_law.rs"]
mod style_effect_kernels_578_p4_law;
#[path = "style_effect_kernels_578_p4_take.rs"]
mod style_effect_kernels_578_p4_take;
#[path = "style_effect_kernels_578_take.rs"]
mod style_effect_kernels_578_take;
#[path = "style_effect_law.rs"]
mod style_effect_law;
#[path = "style_format.rs"]
mod style_format;
#[path = "style_glow_gradient.rs"]
mod style_glow_gradient;
#[path = "style_gradient_fill_layer.rs"]
mod style_gradient_fill_layer;
#[path = "style_gradient_identify_parity.rs"]
mod style_gradient_identify_parity;
#[path = "style_gradient_midpoints.rs"]
mod style_gradient_midpoints;
#[path = "style_gradient_overlay.rs"]
mod style_gradient_overlay;
#[path = "style_gradient_smoothness_align.rs"]
mod style_gradient_smoothness_align;
#[path = "style_inner_glow.rs"]
mod style_inner_glow;
#[path = "style_inner_glow_aa_parity.rs"]
mod style_inner_glow_aa_parity;
#[path = "style_inner_glow_choke100_parity.rs"]
mod style_inner_glow_choke100_parity;
#[path = "style_inner_glow_identify_parity.rs"]
mod style_inner_glow_identify_parity;
#[path = "style_inner_shadow.rs"]
mod style_inner_shadow;
#[path = "style_inside_stroke.rs"]
mod style_inside_stroke;
#[path = "style_noise.rs"]
mod style_noise;
#[path = "style_outer_glow.rs"]
mod style_outer_glow;
#[path = "style_pattern_angle.rs"]
mod style_pattern_angle;
#[path = "style_pattern_overlay.rs"]
mod style_pattern_overlay;
#[path = "style_pattern_reference.rs"]
mod style_pattern_reference;
#[path = "style_pattern_reference_geometry.rs"]
mod style_pattern_reference_geometry;
#[path = "style_pattern_reference_rotate.rs"]
mod style_pattern_reference_rotate;
#[path = "style_pattern_reference_turns.rs"]
mod style_pattern_reference_turns;
#[path = "style_presets.rs"]
mod style_presets;
#[path = "style_satin.rs"]
mod style_satin;
#[path = "style_satin_law.rs"]
mod style_satin_law;
#[path = "style_shape_burst_identify_parity.rs"]
mod style_shape_burst_identify_parity;
#[path = "style_soft_effects_parity.rs"]
mod style_soft_effects_parity;
#[path = "style_stacked.rs"]
mod style_stacked;
#[path = "style_stroke.rs"]
mod style_stroke;
#[path = "style_stroke_aa_parity.rs"]
mod style_stroke_aa_parity;
#[path = "style_stroke_corner_parity.rs"]
mod style_stroke_corner_parity;
#[path = "style_stroke_fill.rs"]
mod style_stroke_fill;
#[path = "style_stroke_pattern.rs"]
mod style_stroke_pattern;
#[path = "style_texture.rs"]
mod style_texture;
#[path = "style_unknown_keys.rs"]
mod style_unknown_keys;
#[path = "style_wide_color.rs"]
mod style_wide_color;
