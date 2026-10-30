//! Closed-form candidate through the unchanged shared alpha/color qualification.
//! Usage: output_root [cropped_reference_directory], or
//!        --full-images output_root full_reference_directory.
use doctored_core::closed_form_matting::{estimate, ClosedFormParams};
use serde_json::json;
use std::path::Path;
#[allow(dead_code)]
#[path = "phase6_full_image_quality.rs"]
mod full;
use full::frozen as shared;

fn candidate(
    _case: &str,
    colors: &[[f32; 3]],
    w: u32,
    h: u32,
    trimap: &[u8],
    prior: &[u8],
) -> Result<shared::AlphaResult, String> {
    let params = ClosedFormParams::default();
    let result = estimate(colors, w, h, trimap, prior, &params, None)?;
    Ok(shared::AlphaResult {
        foreground: None,
        diagnostics: json!({"method":"local color-line Laplacian with hard trimap constraints",
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
    let args: Vec<_> = std::env::args().skip(1).collect();
    let selection = shared::Candidate {
        protect_foreground: false,
        input_policy: shared::InputPolicy::LegacyAutomaticHardLabels,
        name: "closed_form",
        estimate: candidate,
    };
    if args.first().is_some_and(|v| v == "--full-images") {
        full::run(
            Path::new(args.get(1).expect("output root")),
            Path::new(args.get(2).expect("full reference directory")),
            selection,
            2 * 1024 * 1024 * 1024,
        );
    } else {
        shared::run(
            Path::new(args.first().expect("output root")),
            args.get(1).map(Path::new),
            selection,
        );
    }
}
