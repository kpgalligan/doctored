//! Malformed or constraint-violating solver output must be a qualification
//! refusal, never a metric panic or a pass beyond the reported feasible bound.
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};
#[allow(dead_code)]
#[path = "../examples/phase6_matting_quality.rs"]
mod qualification;

fn output(
    alpha: GrayImage,
    confidence: Option<GrayImage>,
) -> Result<qualification::AlphaResult, String> {
    Ok(qualification::AlphaResult {
        foreground: None,
        alpha,
        confidence,
        diagnostics: json!({}),
    })
}

fn evaluate(candidate: qualification::Candidate) -> Value {
    let truth = GrayImage::from_raw(9, 1, vec![0, 0, 0, 0, 128, 255, 255, 255, 255]).unwrap();
    let source = RgbaImage::from_fn(9, 1, |x, _| {
        let a = truth.get_pixel(x, 0)[0];
        image::Rgba([a, a, a, 255])
    });
    qualification::evaluate_candidate(
        (std::path::Path::new("unused"), "contract"),
        &source,
        &truth,
        &[[1.0; 3]; 9],
        "binary",
        false,
        candidate,
    )
}

#[test]
fn wrong_alpha_or_confidence_extent_refuses_before_metrics() {
    let bad_alpha = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::LegacyAutomaticHardLabels,
        name: "test",
        estimate: |_, _, _, _, _, _| output(GrayImage::new(1, 1), None),
    });
    let bad_confidence = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::LegacyAutomaticHardLabels,
        name: "test",
        estimate: |_, _, width, height, _, prior| {
            output(
                GrayImage::from_raw(width, height, prior.to_vec()).unwrap(),
                Some(GrayImage::new(1, 1)),
            )
        },
    });
    for report in [bad_alpha, bad_confidence] {
        assert!(report["alpha_candidate"]["refusal"]
            .as_str()
            .unwrap()
            .contains("dimensions"));
        assert_eq!(report["before"], report["after"]); // Prior is diagnostic only.
        assert_eq!(report["passes_analytic_gate"], false);
        assert_eq!(report["passes_end_to_end_gate"], false);
        assert_eq!(report["estimated_alpha_color"]["passes_color_gate"], false);
    }
}

#[test]
fn known_label_violations_cannot_bypass_the_constraint_audit() {
    let report = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::LegacyAutomaticHardLabels,
        name: "test",
        estimate: |_, _, w, h, trimap, _| {
            assert_eq!(trimap[0], 0);
            assert_eq!(trimap[8], 255);
            output(GrayImage::new(w, h), None)
        },
    });
    assert!(report["alpha_candidate"]["refusal"]
        .as_str()
        .unwrap()
        .contains("known categorical"));
    assert_eq!(report["passes_analytic_gate"], false);
    assert_eq!(report["passes_end_to_end_gate"], false);
    assert_eq!(
        report["estimated_alpha_color"]["alpha_candidate_refused"],
        true
    );
}

#[test]
fn conforming_candidate_remains_eligible() {
    // Independently observed RGB equals alpha in this white-over-black fixture.
    let report = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::LegacyAutomaticHardLabels,
        name: "test",
        estimate: |_, colors, w, h, _, _| {
            output(
                GrayImage::from_raw(
                    w,
                    h,
                    colors
                        .iter()
                        .map(|c| (c[0] * 255.0).round() as u8)
                        .collect(),
                )
                .unwrap(),
                None,
            )
        },
    });
    assert!(report["alpha_candidate"].get("refusal").is_none());
    assert_eq!(report["passes_analytic_gate"], true);
    assert_eq!(report["after"]["boundary_mad"], 0.0);
}

#[test]
fn soft_policy_does_not_pass_automatic_labels_as_user_constraints() {
    let report = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::AutomaticSoftPriorV1,
        name: "soft-policy-test",
        estimate: |_, _, w, h, user, prior| {
            assert!(user.iter().all(|&value| value == 128));
            assert_eq!(prior[0], 0);
            assert_eq!(prior[8], 255);
            output(GrayImage::from_pixel(w, h, image::Luma([64])), None)
        },
    });
    assert!(report["alpha_candidate"].get("refusal").is_none());
    assert_eq!(report["input_policy"]["version"], 1);
    assert_eq!(
        report["input_policy"]["automatic_labels_used_as_hard_constraints"],
        false
    );
    assert!(report.get("legacy_automatic_constraint_audit").is_some());
    // Eligibility does not excuse inaccurate output from the original gates.
    assert_eq!(report["passes_analytic_gate"], false);
    assert_eq!(report["passes_end_to_end_gate"], false);
}

#[test]
fn invalid_joint_foreground_refuses_without_scoring_or_publication() {
    for mode in 0..3 {
        let candidate: qualification::EstimateAlpha = match mode {
            0 => |_, _, w, h, _, prior| {
                let mut result = output(GrayImage::from_raw(w, h, prior.to_vec()).unwrap(), None)?;
                result.foreground = Some(vec![[0.0; 3]; 1]);
                Ok(result)
            },
            1 => |_, _, w, h, _, prior| {
                let mut result = output(GrayImage::from_raw(w, h, prior.to_vec()).unwrap(), None)?;
                result.foreground = Some(vec![[f32::NAN; 3]; (w * h) as usize]);
                Ok(result)
            },
            _ => |_, _, w, h, _, prior| {
                let mut result = output(GrayImage::from_raw(w, h, prior.to_vec()).unwrap(), None)?;
                result.foreground = Some(vec![[1.01; 3]; (w * h) as usize]);
                Ok(result)
            },
        };
        let report = evaluate(qualification::Candidate {
            protect_foreground: false,
            input_policy: qualification::InputPolicy::AutomaticSoftPriorV1,
            name: "bad-joint",
            estimate: candidate,
        });
        assert!(report["alpha_candidate"]["refusal"]
            .as_str()
            .unwrap()
            .contains("foreground"));
        assert_eq!(report["before"], report["after"]);
        assert_eq!(report["passes_end_to_end_gate"], false);
    }
}

#[test]
fn joint_foreground_is_scored_directly_with_independent_source_equation() {
    let report = evaluate(qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::AutomaticSoftPriorV1,
        name: "source-joint",
        estimate: |_, colors, w, h, _, _| {
            // This source was independently composed white over black: C=alpha.
            let mut result = output(
                GrayImage::from_raw(
                    w,
                    h,
                    colors
                        .iter()
                        .map(|c| (c[0] * 255.0).round() as u8)
                        .collect(),
                )
                .unwrap(),
                None,
            )?;
            result.foreground = Some(vec![[1.0; 3]; colors.len()]);
            Ok(result)
        },
    });
    assert!(report["alpha_candidate"].get("refusal").is_none());
    assert_eq!(report["after"]["boundary_mad"], 0.0);
    assert_eq!(
        report["estimated_alpha_color"]["after"]["fractional_foreground_mad"],
        0.0
    );
    assert_eq!(report["passes_end_to_end_gate"], true);
    assert!(report.get("joint_foreground_pipeline").is_some());
}
