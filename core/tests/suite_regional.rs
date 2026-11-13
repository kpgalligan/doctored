//! The regional render (`regional_*`) and bounded-support (`bounded_*`) test files as one binary.
#![allow(clippy::duplicate_mod)]

mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/region_plan.rs"]
mod region_plan;
#[path = "common/resource_report.rs"]
mod resource_report;

#[path = "bounded_artwork_coverage.rs"]
mod bounded_artwork_coverage;
#[path = "bounded_brush_footprint.rs"]
mod bounded_brush_footprint;
#[path = "bounded_selection_support.rs"]
mod bounded_selection_support;
#[path = "job_control_park.rs"]
mod job_control_park;
#[path = "regional_byte_clip_guide.rs"]
mod regional_byte_clip_guide;
#[path = "regional_byte_effects.rs"]
mod regional_byte_effects;
#[path = "regional_byte_guide.rs"]
mod regional_byte_guide;
#[path = "regional_byte_knockout.rs"]
mod regional_byte_knockout;
#[path = "regional_byte_nested_guide.rs"]
mod regional_byte_nested_guide;
#[path = "regional_byte_pass_through_guide.rs"]
mod regional_byte_pass_through_guide;
#[path = "regional_byte_preparation_cancel.rs"]
mod regional_byte_preparation_cancel;
#[path = "regional_byte_primitive_cancel.rs"]
mod regional_byte_primitive_cancel;
#[path = "regional_byte_styled_clip_guide.rs"]
mod regional_byte_styled_clip_guide;
#[path = "regional_byte_styled_groups.rs"]
mod regional_byte_styled_groups;
#[path = "regional_byte_styles.rs"]
mod regional_byte_styles;
#[path = "regional_context_preparation_cancel.rs"]
mod regional_context_preparation_cancel;
#[path = "regional_document.rs"]
mod regional_document;
#[path = "regional_group_materialization_cancel.rs"]
mod regional_group_materialization_cancel;
#[path = "regional_groups.rs"]
mod regional_groups;
#[path = "regional_masks_clips.rs"]
mod regional_masks_clips;
#[path = "regional_precision.rs"]
mod regional_precision;
#[path = "regional_precision_clip_guide.rs"]
mod regional_precision_clip_guide;
#[path = "regional_precision_groups.rs"]
mod regional_precision_groups;
#[path = "regional_precision_guide.rs"]
mod regional_precision_guide;
#[path = "regional_precision_knockout.rs"]
mod regional_precision_knockout;
#[path = "regional_precision_masks_clips.rs"]
mod regional_precision_masks_clips;
#[path = "regional_precision_nested_guide.rs"]
mod regional_precision_nested_guide;
#[path = "regional_precision_pass_through_guide.rs"]
mod regional_precision_pass_through_guide;
#[path = "regional_precision_styled_clip_guide.rs"]
mod regional_precision_styled_clip_guide;
#[path = "regional_precision_styled_groups.rs"]
mod regional_precision_styled_groups;
#[path = "regional_precision_styles.rs"]
mod regional_precision_styles;
#[path = "regional_resource_accounting.rs"]
mod regional_resource_accounting;
#[path = "regional_run_path.rs"]
mod regional_run_path;
#[path = "regional_sampling_fallback_cancel.rs"]
mod regional_sampling_fallback_cancel;
#[path = "regional_sampling_tree_cancel.rs"]
mod regional_sampling_tree_cancel;
#[path = "regional_style_preparation_cancel.rs"]
mod regional_style_preparation_cancel;
#[path = "regional_typed_preparation_cancel.rs"]
mod regional_typed_preparation_cancel;
#[path = "regional_typed_styled_composite_cancel.rs"]
mod regional_typed_styled_composite_cancel;
