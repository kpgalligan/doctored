//! Predeclared soft-prior candidate; original evaluator, fixtures and gates.
use doctored_core::{
    closed_form_matting::{estimate_soft_prior, ClosedFormParams},
    matting_input::SoftPriorInput,
};
use serde_json::json;
use std::path::Path;
#[allow(dead_code)]
#[path = "phase6_full_image_quality.rs"]
mod full;
use full::frozen as shared;

fn candidate(
    _case: &str,
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    user_constraints: &[u8],
    prior: &[u8],
) -> Result<shared::AlphaResult, String> {
    let params = ClosedFormParams::default();
    let input = SoftPriorInput {
        version: 1,
        user_constraints: Some(user_constraints),
        // A window contributes roughly unit diagonal per covering position.
        // One hundredth puts image structure ahead of the uncertain prior,
        // while removing the nullspace even without an opaque glass sample.
        prior_weight: 0.01,
    };
    let result = estimate_soft_prior(colors, (width, height), prior, input, &params, None)?;
    Ok(shared::AlphaResult {
        foreground: None,
        diagnostics: json!({"method":"shared closed-form Laplacian with version1 soft automatic prior",
            "input_policy_version":1,"prior_weight":input.prior_weight,
            "unknown_pixels":result.unknown_pixels,"active_windows":result.active_windows,
            "iterations":result.iterations,"residual_rms":result.residual_rms,
            "relative_residual":result.relative_residual,"clipped_pixels":result.clipped_pixels,
            "numerically_converged":true,"identifiability_confidence_available":false,
            "parameters":{"epsilon":params.epsilon,"max_iterations":params.max_iterations,
                "residual_rms_tolerance":params.residual_rms_tolerance}}),
        alpha: result.alpha,
        confidence: None,
    })
}

fn main() {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let stable = args.first().is_some_and(|v| v == "--stable-prior");
    let protected = stable || args.first().is_some_and(|v| v == "--protected");
    let tiled = protected || args.first().is_some_and(|v| v == "--tiled");
    if tiled {
        args.remove(0);
    }
    let selection = shared::Candidate {
        protect_foreground: protected,
        name: if stable {
            "stable_prior_protected_v4"
        } else if protected {
            "protected_tiled_soft_prior_v3"
        } else if tiled {
            "tiled_soft_prior_v2"
        } else {
            "soft_prior_v1"
        },
        input_policy: shared::InputPolicy::AutomaticSoftPriorV1,
        estimate: if stable {
            stable_candidate
        } else if tiled {
            tiled_candidate
        } else {
            candidate
        },
    };
    let root = Path::new(
        args.get(1)
            .expect("output root after analytic or full mode"),
    );
    assert!(!root.exists(), "new output directory required");
    match args.first().map(String::as_str) {
        Some("analytic") => shared::run(root, args.get(2).map(Path::new), selection),
        Some("full") => full::run(
            root,
            Path::new(args.get(2).expect("full references")),
            selection,
            2 * 1024 * 1024 * 1024,
        ),
        _ => panic!("expected analytic or full mode"),
    }
}

fn stable_candidate(
    _case: &str,
    _colors: &[[f32; 3]],
    width: u32,
    height: u32,
    user_constraints: &[u8],
    prior: &[u8],
) -> Result<shared::AlphaResult, String> {
    if prior.len() != width as usize * height as usize || user_constraints.len() != prior.len() {
        return Err("stable prior dimensions do not match input planes".into());
    }
    let mut alpha = prior.to_vec();
    let mut changed = 0;
    for (value, &label) in alpha.iter_mut().zip(user_constraints) {
        match label {
            128 => {}
            0 | 255 => {
                changed += usize::from(*value != label);
                *value = label;
            }
            _ => return Err("stable prior user labels must be categorical".into()),
        }
    }
    Ok(shared::AlphaResult {
        foreground: None,
        alpha: image::GrayImage::from_raw(width, height, alpha)
            .ok_or("invalid stable prior extent")?,
        diagnostics: json!({"method":"unchanged supplied prior except explicit user constraints",
            "input_policy_version":1,"explicit_user_changed_pixels":changed,
            "automatic_alpha_estimation":false,"physical_opacity_claimed":false}),
        confidence: None,
    })
}

fn tiled_candidate(
    _case: &str,
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    user_constraints: &[u8],
    prior: &[u8],
) -> Result<shared::AlphaResult, String> {
    let params = ClosedFormParams::default();
    let input = SoftPriorInput {
        version: 1,
        user_constraints: Some(user_constraints),
        prior_weight: 1.0,
    };
    let result = doctored_core::tiled_soft_matting::estimate(
        colors,
        (width, height),
        prior,
        input,
        &params,
        None,
    )?;
    Ok(shared::AlphaResult {
        foreground: None,
        diagnostics: json!({"method":"shared closed-form local tile objectives with version1 soft automatic prior",
            "input_policy_version":1,"prior_weight":input.prior_weight,
            "tile_output_side":256,"tile_halo":16,"tile_count":result.tile_count,
            "processed_tile_pixels":result.processed_tile_pixels,
            "iteration_limit":result.iteration_limit,"iterations_total":result.iterations_total,
            "maximum_tile_residual_rms":result.maximum_tile_residual_rms,
            "estimated_stencil_terms":result.estimated_stencil_terms,
            "all_local_systems_converged":true,"global_system_convergence_claimed":false,
            "identifiability_confidence_available":false,
            "parameters":{"epsilon":params.epsilon,"max_iterations_before_work_admission":params.max_iterations,
                "residual_rms_tolerance":params.residual_rms_tolerance}}),
        alpha: result.alpha,
        confidence: None,
    })
}
