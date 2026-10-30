//! The three RE-PREPARATION plans — 8→16 structure promotion, profile
//! assign/convert (`color_plan`) and the float→16 narrowing (in
//! `precision_conversion_narrow.rs`, beside its fixture) — charge adjustment
//! records through the one `AdjustmentAdmission` rule the native writer and
//! reader share: each record's measured retention plus ONE peak preparation
//! allowance, not one allowance per record.
//!
//! The oracle is the plan DELTA per added record, bounded independently:
//! below one preparation allowance (`PrecisionAdjustmentState::
//! prepare_scratch_bytes`, whose constant floor alone is 4 MiB) and, for a
//! calibrated operator that encodes (White Balance), at least its ENCODER
//! tables — three 65,537-entry f64 tables, which every prepared White Balance
//! holds (a calibrated Exposure holds none since #2: one exponent per
//! channel). A
//! conversion reuses the records `rebind_layers` prepared, so its
//! delta equals assignment's. A 600-adjustment stack, which one allowance per
//! record (≥ 2.4 GiB) put past `REFERENCE_WORKING_BYTES`, now completes all three.
//!
//! The 8→16 plan walk admits each adjustment preparation BEFORE it
//! builds it, so a limit too small for one preparation refuses from the plan
//! at the preparation's own total, below the whole operation's figure.
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_adjustment::PrecisionAdjustmentState;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use image::RgbaImage;
use serde_json::json;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// Three 65,537-entry f64 forward-encoder tables.
const ENC: u64 = 3 * 65_537 * 8;
/// The plan's bookkeeping, and the constant floor of one preparation
/// allowance (`PrecisionAdjustmentState::prepare_scratch_bytes`).
const MIB4: u64 = 4 * 1024 * 1024;

fn white_balance() -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"white_balance",
           "params":{"temperature":5200}})
    .to_string()
}

fn typed(adjustments: usize) -> PrecisionDocument {
    let pixels = (0..64_u16)
        .map(|i| [i * 1000, 65535 - i * 900, i * 500, 65535])
        .collect::<Vec<_>>();
    let image = PrecisionImage::from_rgba16(8, 8, pixels, IccProfile::srgb(), None).unwrap();
    let mut doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    for _ in 0..adjustments {
        doc = doc
            .add_adjustment("White Balance", &white_balance(), 1.0, LIMIT, 0, None)
            .unwrap()
            .unwrap();
    }
    doc
}

fn byte(levels: usize) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(8, 8, |x, y| {
        image::Rgba([(x * 30) as u8, (y * 30) as u8, 90, 255])
    }));
    for k in 0..levels {
        let mut layer = DrDocument::from_pixels(RgbaImage::new(1, 1))
            .layers
            .remove(0);
        layer.name = format!("Levels {k}");
        layer.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#.into());
        doc.layers.push(layer);
    }
    doc
}

fn allowance(profile: &Arc<IccProfile>) -> u64 {
    PrecisionAdjustmentState::prepare_scratch_bytes(&white_balance(), profile).unwrap()
}

/// `N` from `admit`'s "precision operation needs N bytes …" sentence.
fn needs(refusal: &str) -> u64 {
    refusal
        .strip_prefix("precision operation needs ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("not an admission refusal: {refusal}"))
}

#[test]
fn a_long_adjustment_stack_promotes_assigns_and_converts() {
    let doc = byte(600);
    let plan = doc.promote_structure_plan().unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    let sixteen = doc.promote_structure(LIMIT, 0, None).unwrap();
    assert_eq!(sixteen.layer_count(), 601);
    let p3 = IccProfile::display_p3();
    let assigned = sixteen
        .assign_profile(&p3, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("assign refused: {e}"));
    assert!(assigned.is_some());
    let converted = sixteen
        .convert_to_profile(&p3, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("convert refused: {e}"))
        .expect("an edit");
    // The reused rebound records keep every description.
    for i in 0..sixteen.layer_count() {
        assert_eq!(
            converted.adjustment_description(i),
            sixteen.adjustment_description(i),
            "record {i}"
        );
    }
}

#[test]
fn each_added_white_balance_charges_its_record_not_an_allowance_in_color_plan() {
    let p3 = IccProfile::display_p3();
    let below = allowance(&p3);
    let (one, two) = (typed(1), typed(2));
    let mut deltas = Vec::new();
    for convert in [false, true] {
        let delta = two.color_plan(&p3, convert).unwrap().scratch_bytes
            - one.color_plan(&p3, convert).unwrap().scratch_bytes;
        assert!(
            delta >= ENC,
            "convert {convert}: delta {delta} is below the rebound record {ENC}"
        );
        assert!(
            delta < below,
            "convert {convert}: delta {delta} is not below one allowance {below}"
        );
        deltas.push(delta);
    }
    // A conversion charges the rebound records once, as assignment does.
    assert_eq!(deltas[1], deltas[0], "convert delta != assign delta");
}

/// With room for the retained input and the bookkeeping but not one
/// preparation allowance, the 8→16 op refuses from its plan walk — at the
/// retained input plus the allowance (≥ its 4 MiB floor) plus the 4 MiB
/// bookkeeping, and below the whole-operation figure the op used to refuse
/// with.
#[test]
fn planning_admits_each_preparation_before_building_it() {
    let doc = byte(1);
    let plan = doc.promote_structure_plan().unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    let whole = plan.retained_bytes + plan.scratch_bytes + plan.output_bytes;
    let floor = plan.retained_bytes + 2 * MIB4;
    let refusal = doc
        .promote_structure(plan.retained_bytes + MIB4, 0, None)
        .err()
        .expect("refused");
    let n = needs(&refusal);
    assert!(n < whole, "{n} is not below the whole operation's {whole}");
    assert!(
        n >= floor,
        "{n} is below retained + allowance + bookkeeping {floor}"
    );
}

/// The adversary's case: the full-size placeholder a retained-source
/// raster holds in the plan walk is still live when a later adjustment is
/// prepared, so the preparation's admission counts it.
#[test]
fn planning_counts_earlier_source_shapes_while_preparing() {
    let master = DrDocument::from_pixels(RgbaImage::new(512, 512));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"185-master","revision_id":"v1",
                "name":"Retained","x":0,"y":0})
        .to_string(),
    )
    .unwrap();
    let mut doc = byte(0)
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let sourced = doc.layers.len() - 1;
    assert!(doc.layers[sourced].effective_source(&doc.profile).is_some());
    let (w, h) = doc.layers[sourced].pixels.dimensions();
    let shape = u64::from(w) * u64::from(h) * 8;
    let mut levels = byte(1).layers.remove(1);
    levels.name = "Levels above".into();
    doc.layers.push(levels);
    let plan = doc.promote_structure_plan().unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    let floor = plan.retained_bytes + 2 * MIB4 + shape;
    let refusal = doc
        .promote_structure(plan.retained_bytes + MIB4, 0, None)
        .err()
        .expect("refused");
    let n = needs(&refusal);
    assert!(
        n >= floor,
        "{n} does not count the {shape}-byte source shape"
    );
}

#[test]
fn each_added_levels_charges_its_record_not_an_allowance_in_promotion() {
    let delta = byte(2).promote_structure_plan().unwrap().scratch_bytes
        - byte(1).promote_structure_plan().unwrap().scratch_bytes;
    assert!(delta > 0, "the added record is not charged");
    assert!(
        delta < 4 * 1024 * 1024,
        "delta {delta} is not below one allowance's constant floor"
    );
}
