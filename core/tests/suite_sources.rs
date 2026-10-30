//! The filter, RAW and retained-source test files as one binary: `live_*`, `raw_*`, `retained_*`, `linked_source_*`, `host_source_*`, `source_*`, `single_image_capacity`, `persisted_warp_*`, `puppet_warp_*`, `perspective_warp_*`, `filter_*`, and the filter files that belong with them.
#![allow(clippy::duplicate_mod)]

mod common;
#[path = "common/develop_filter_reference.rs"]
mod develop_filter_reference;
#[path = "common/extended_box_reference.rs"]
mod extended_box_reference;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/gaussian_reference.rs"]
mod gaussian_reference;
#[path = "common/group_roots.rs"]
mod group_roots;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;
#[path = "common/identity.rs"]
mod identity;
mod live_filter_support;
#[path = "common/native_blend_oracle.rs"]
mod native_blend_oracle;
#[path = "common/raw_geometry_fixture.rs"]
mod raw_geometry_fixture;
#[path = "common/resource_report.rs"]
mod resource_report;
#[path = "common/smart_sharpen_reference.rs"]
mod smart_sharpen_reference;
#[path = "common/source_refusal_fixture.rs"]
mod source_refusal_fixture;
#[path = "common/vibrance_reference.rs"]
mod vibrance_reference;
#[allow(dead_code)]
#[path = "common/warp_fixture.rs"]
mod warp_fixture;
#[path = "common/warp_oracle.rs"]
mod warp_oracle;

#[path = "banded_reduce_noise.rs"]
mod banded_reduce_noise;
#[path = "cleanup_filters.rs"]
mod cleanup_filters;
#[path = "develop_inspection.rs"]
mod develop_inspection;
#[path = "develop_preview_extended.rs"]
mod develop_preview_extended;
#[path = "filter_depth_blur.rs"]
mod filter_depth_blur;
#[path = "filter_depth_parity.rs"]
mod filter_depth_parity;
#[path = "filter_develop_filter.rs"]
mod filter_develop_filter;
#[path = "filter_develop_filter_masks.rs"]
mod filter_develop_filter_masks;
#[path = "filter_job_work_cap.rs"]
mod filter_job_work_cap;
#[path = "filter_lens_correction.rs"]
mod filter_lens_correction;
#[path = "filter_lens_profiles.rs"]
mod filter_lens_profiles;
#[path = "filter_liquify.rs"]
mod filter_liquify;
#[path = "filter_motion_lens_tilt.rs"]
mod filter_motion_lens_tilt;
#[path = "filter_presence.rs"]
mod filter_presence;
#[path = "filter_presence_scale.rs"]
mod filter_presence_scale;
#[path = "filter_preview_cancel.rs"]
mod filter_preview_cancel;
#[path = "filter_smart_sharpen.rs"]
mod filter_smart_sharpen;
#[path = "filter_tests.rs"]
mod filter_tests;
#[path = "gaussian_blur_photoshop.rs"]
mod gaussian_blur_photoshop;
#[path = "gaussian_radius_independent.rs"]
mod gaussian_radius_independent;
#[path = "host_source_packing.rs"]
mod host_source_packing;
#[path = "linked_source_capacity.rs"]
mod linked_source_capacity;
#[path = "linked_source_native.rs"]
mod linked_source_native;
#[path = "linked_source_ops.rs"]
mod linked_source_ops;
#[path = "live_gaussian_untruncated.rs"]
mod live_gaussian_untruncated;
#[path = "persisted_warp_native.rs"]
mod persisted_warp_native;
#[path = "persisted_warp_numeric.rs"]
mod persisted_warp_numeric;
#[path = "persisted_warp_safety.rs"]
mod persisted_warp_safety;
#[path = "perspective_warp.rs"]
mod perspective_warp;
#[path = "perspective_warp_depths.rs"]
mod perspective_warp_depths;
#[path = "phase8e_coverage.rs"]
mod phase8e_coverage;
#[path = "phase8e_native.rs"]
mod phase8e_native;
#[path = "projective_source_native.rs"]
mod projective_source_native;
#[path = "puppet_warp.rs"]
mod puppet_warp;
#[path = "puppet_warp_depths.rs"]
mod puppet_warp_depths;
#[path = "radial_clouds_offset.rs"]
mod radial_clouds_offset;
#[path = "raw_descriptor_numbers.rs"]
mod raw_descriptor_numbers;
#[path = "raw_float_develop.rs"]
mod raw_float_develop;
#[path = "raw_float_grain.rs"]
mod raw_float_grain;
#[path = "raw_lensfun_locals.rs"]
mod raw_lensfun_locals;
#[path = "raw_local_capacity.rs"]
mod raw_local_capacity;
#[path = "raw_local_native.rs"]
mod raw_local_native;
#[path = "raw_local_numeric.rs"]
mod raw_local_numeric;
#[path = "raw_local_range.rs"]
mod raw_local_range;
#[path = "raw_local_resize.rs"]
mod raw_local_resize;
#[path = "raw_local_tone.rs"]
mod raw_local_tone;
#[path = "raw_source_geometry_native.rs"]
mod raw_source_geometry_native;
#[path = "raw_source_geometry_numeric.rs"]
mod raw_source_geometry_numeric;
#[path = "raw_source_geometry_safety.rs"]
mod raw_source_geometry_safety;
#[path = "raw_source_native.rs"]
mod raw_source_native;
#[path = "raw_source_ops.rs"]
mod raw_source_ops;
#[path = "raw_vignette_geometry.rs"]
mod raw_vignette_geometry;
#[path = "removed_feature_files.rs"]
mod removed_feature_files;
#[path = "retained_geometry.rs"]
mod retained_geometry;
#[path = "retained_source_native.rs"]
mod retained_source_native;
#[path = "retained_source_work.rs"]
mod retained_source_work;
#[path = "retained_sources.rs"]
mod retained_sources;
#[path = "selected_adjustments.rs"]
mod selected_adjustments;
#[path = "selected_banded_filters.rs"]
mod selected_banded_filters;
#[path = "sharpen_filters.rs"]
mod sharpen_filters;
#[path = "single_image_capacity.rs"]
mod single_image_capacity;
#[path = "source_conversion_lut.rs"]
mod source_conversion_lut;
#[path = "source_conversion_policy.rs"]
mod source_conversion_policy;
#[path = "source_locks.rs"]
mod source_locks;
#[path = "source_profile_refusal.rs"]
mod source_profile_refusal;
#[path = "spatial_filters.rs"]
mod spatial_filters;
