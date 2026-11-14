//! The retouch, matting, selection and paint test files as one binary: `retouch_*`, `content_*`, `matting_*`, `bayesian_*`, `foreground_*`, `assisted_*`, `transform_*`, `doc_*`, and the files that belong with them.
#![allow(clippy::duplicate_mod)]

#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
#[path = "common/cmyk_structure_fixture.rs"]
mod cmyk_structure_fixture;
#[allow(dead_code)]
#[path = "common/cmyk_tiff_builder.rs"]
mod cmyk_tiff_builder;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/matting_quality.rs"]
mod matting_quality;
#[path = "common/native_scalar.rs"]
mod native_scalar;
#[path = "common/resource_report.rs"]
mod resource_report;
#[path = "common/rgba_layer.rs"]
mod rgba_layer;
#[path = "common/assisted_scoring.rs"]
mod scoring;
#[path = "common/vibrance_reference.rs"]
mod vibrance_reference;

#[path = "alpha_sampling.rs"]
mod alpha_sampling;
#[path = "art_history_brush.rs"]
mod art_history_brush;
#[path = "art_history_refit.rs"]
mod art_history_refit;
#[path = "assisted_replay.rs"]
mod assisted_replay;
#[path = "assisted_scoring.rs"]
mod assisted_scoring;
#[path = "bayesian_color.rs"]
mod bayesian_color;
#[path = "bayesian_matting.rs"]
mod bayesian_matting;
#[path = "closed_form_matting.rs"]
mod closed_form_matting;
#[path = "color_range_gamut.rs"]
mod color_range_gamut;
#[path = "color_range_presets.rs"]
mod color_range_presets;
#[path = "content_aware_admission.rs"]
mod content_aware_admission;
#[path = "content_aware_cost_tests.rs"]
mod content_aware_cost_tests;
#[path = "content_aware_fill_transparent.rs"]
mod content_aware_fill_transparent;
#[path = "content_aware_large.rs"]
mod content_aware_large;
#[path = "content_aware_photographs.rs"]
mod content_aware_photographs;
#[path = "content_aware_tests.rs"]
mod content_aware_tests;
#[path = "content_aware_workflow.rs"]
mod content_aware_workflow;
#[path = "content_bounds_parity.rs"]
mod content_bounds_parity;
#[path = "content_bounds_rows.rs"]
mod content_bounds_rows;
#[path = "content_box_tests.rs"]
mod content_box_tests;
#[path = "content_tests.rs"]
mod content_tests;
#[path = "doc_gradient_stops.rs"]
mod doc_gradient_stops;
#[path = "doc_paint.rs"]
mod doc_paint;
#[path = "doc_retouch.rs"]
mod doc_retouch;
#[path = "edge_refine.rs"]
mod edge_refine;
#[path = "edge_trace.rs"]
mod edge_trace;
#[path = "external_matte_inputs.rs"]
mod external_matte_inputs;
#[path = "fade.rs"]
mod fade;
#[path = "fill_stroke_tests.rs"]
mod fill_stroke_tests;
#[path = "focus_area.rs"]
mod focus_area;
#[path = "foreground_ambiguity.rs"]
mod foreground_ambiguity;
#[path = "foreground_estimation.rs"]
mod foreground_estimation;
#[path = "foreground_protection.rs"]
mod foreground_protection;
#[path = "heal_job.rs"]
mod heal_job;
#[path = "heal_tests.rs"]
mod heal_tests;
#[path = "job_control.rs"]
mod job_control;
#[path = "matting_candidate_contract.rs"]
mod matting_candidate_contract;
#[path = "matting_constraint_audit.rs"]
mod matting_constraint_audit;
#[path = "matting_qualification.rs"]
mod matting_qualification;
#[path = "matting_soft_prior.rs"]
mod matting_soft_prior;
#[path = "move_selected_pixels.rs"]
mod move_selected_pixels;
#[path = "paint_walk.rs"]
mod paint_walk;
#[path = "pixel_art.rs"]
mod pixel_art;
#[path = "plane_ink.rs"]
mod plane_ink;
#[path = "redeye_tests.rs"]
mod redeye_tests;
#[path = "refinement_cutout.rs"]
mod refinement_cutout;
#[path = "refinement_execution.rs"]
mod refinement_execution;
#[path = "retouch_brush.rs"]
mod retouch_brush;
#[path = "retouch_brush_options.rs"]
mod retouch_brush_options;
#[path = "retouch_ffi.rs"]
mod retouch_ffi;
#[path = "retouch_output.rs"]
mod retouch_output;
#[path = "retouch_output_scratch.rs"]
mod retouch_output_scratch;
#[path = "retouch_protect_tones.rs"]
mod retouch_protect_tones;
#[path = "retouch_sampling.rs"]
mod retouch_sampling;
#[path = "sampled_matting.rs"]
mod sampled_matting;
#[path = "select_similar.rs"]
mod select_similar;
#[path = "selected_pixels_float.rs"]
mod selected_pixels_float;
#[path = "selection_quick.rs"]
mod selection_quick;
#[path = "selection_tests.rs"]
mod selection_tests;
#[path = "selection_tools.rs"]
mod selection_tools;
#[path = "spot_heal_types.rs"]
mod spot_heal_types;
#[path = "stroke_layer_edge.rs"]
mod stroke_layer_edge;
#[path = "tiled_soft_matting.rs"]
mod tiled_soft_matting;
#[path = "transform_selected_color_planes.rs"]
mod transform_selected_color_planes;
#[path = "transform_selected_pixels.rs"]
mod transform_selected_pixels;
#[path = "transform_selected_planes.rs"]
mod transform_selected_planes;
#[path = "transform_tests.rs"]
mod transform_tests;
#[path = "trimap_strokes.rs"]
mod trimap_strokes;
#[path = "wand_layer_tests.rs"]
mod wand_layer_tests;
#[path = "wand_predicate.rs"]
mod wand_predicate;
#[path = "wand_sample_size.rs"]
mod wand_sample_size;
