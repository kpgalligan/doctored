//! Frozen joint Bayesian research candidate; unchanged evaluator and90 cases.
use doctored_core::bayesian_matting;
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
    user: &[u8],
    prior: &[u8],
) -> Result<shared::AlphaResult, String> {
    let result = bayesian_matting::estimate(
        colors,
        (width, height),
        prior,
        user,
        2 * 1024 * 1024 * 1024,
        None,
    )?;
    Ok(shared::AlphaResult {
        alpha: result.alpha,
        foreground: Some(result.foreground),
        confidence: None,
        diagnostics: json!({"method":"joint Bayesian alpha/F/B with uncertain source-derived color distributions",
            "input_policy_version":1,"automatic_support_is_physical_truth":false,
            "foreground_samples":result.foreground_samples,"background_samples":result.background_samples,
            "automatic_foreground_support":result.automatic_foreground_support,"automatic_background_support":result.automatic_background_support,
            "joint_evaluations":result.joint_evaluations,"spatial_node_visits":result.spatial_node_visits,
            "alpha_changed_pixels":result.alpha_changed_pixels,"high_prior_alpha_changed_pixels":result.high_prior_alpha_changed_pixels,
            "foreground_changed_pixels":result.foreground_changed_pixels,"convergence_claimed":false}),
    })
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let selection = shared::Candidate {
        protect_foreground: false,
        name: "joint_bayesian_v5",
        input_policy: shared::InputPolicy::AutomaticSoftPriorV1,
        estimate: candidate,
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
