//! The audit is an impossibility bound, not truth-assisted candidate input.
use crate::matting_quality as quality;
use image::GrayImage;

#[test]
fn counterfactual_bound_equals_exhaustive_unknown_search() {
    // One opaque constraint, one wrongly hidden soft pixel, one unknown. Vary
    // the unknown over every byte independently of the audit implementation.
    let truth = GrayImage::from_raw(3, 1, vec![255, 128, 64]).unwrap();
    let trimap = GrayImage::from_raw(3, 1, vec![255, 0, 128]).unwrap();
    let prior = GrayImage::from_raw(3, 1, vec![255, 0, 0]).unwrap();
    let before = quality::alpha_metrics(&truth, &prior);
    let audit = quality::constraint_audit(&truth, &trimap, &before);
    let mut smallest_mad = f64::INFINITY;
    let mut largest_recall = 0.0_f64;
    for value in 0..=255 {
        let candidate = GrayImage::from_raw(3, 1, vec![255, 0, value]).unwrap();
        let measured = quality::alpha_metrics(&truth, &candidate);
        smallest_mad = smallest_mad.min(measured["boundary_mad"].as_f64().unwrap());
        largest_recall = largest_recall.max(measured["soft_detail_recall"].as_f64().unwrap());
        assert!(!quality::alpha_gate(&before, &measured));
    }
    assert_eq!(audit["minimum_possible_boundary_mad"], smallest_mad);
    assert_eq!(audit["maximum_possible_soft_detail_recall"], largest_recall);
    assert_eq!(largest_recall, 0.5);
    assert_eq!(audit["incorrect_known_background_pixels"], 1);
    assert_eq!(audit["alpha_gate_feasible_under_known_constraints"], false);
    assert_eq!(trimap.as_raw(), &[255, 0, 128]);
}

#[test]
fn feasible_constraints_do_not_claim_candidate_success() {
    let truth = GrayImage::from_raw(3, 1, vec![0, 128, 255]).unwrap();
    let trimap = GrayImage::from_raw(3, 1, vec![0, 128, 255]).unwrap();
    let poor = GrayImage::from_raw(3, 1, vec![0, 0, 255]).unwrap();
    let before = quality::alpha_metrics(&truth, &poor);
    let audit = quality::constraint_audit(&truth, &trimap, &before);
    assert_eq!(audit["alpha_gate_feasible_under_known_constraints"], true);
    assert_eq!(audit["minimum_possible_boundary_mad"], 0.0);
    assert_eq!(audit["maximum_possible_soft_detail_recall"], 1.0);
    assert!(!quality::alpha_gate(&before, &before));
}
