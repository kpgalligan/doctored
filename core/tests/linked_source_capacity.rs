//! Future source/fallback capacity is compared with the original document, not
//! with a hypothetical document that has already incorporated the proposed growth.
use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::json;
fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([31, 63, 127, 255])))
}
fn place(convert: bool) -> SourceEdit {
    SourceEdit::parse(&json!({"op":"place","source_id":"source","name":"Photo","x":0,"y":0,"depth_conversion":if convert{"convert_fallback"}else{"reject"}}).to_string()).unwrap()
}
fn fill_layers(source: &DrDocument, count: usize) -> DrDocument {
    let mut doc = source.clone();
    doc.layers = vec![source.layers[0].clone(); count];
    doc
}
#[test]
fn source_placement_refuses_record_growth_before_any_pixel_render() {
    let source = byte(1, 1);
    let full = fill_layers(&source, 1024);
    assert!(full
        .source_plan(Some(&source), &place(false))
        .err()
        .unwrap()
        .contains("native"));
    let room = fill_layers(&source, 1023);
    assert!(room.source_plan(Some(&source), &place(false)).is_ok());
    // A previously oversized state cannot gain another source record either.
    let oversized = fill_layers(&source, 1025);
    assert!(oversized.source_plan(Some(&source), &place(false)).is_err());
}
#[test]
fn placement_counts_new_fallback_and_genuine_master_against_original_weighted_total() {
    // Hundreds of serialized records share one real 4 MB source buffer.
    let source = byte(1000, 1000);
    assert!(fill_layers(&source, 398)
        .source_plan(Some(&source), &place(false))
        .is_ok());
    assert!(fill_layers(&source, 399)
        .source_plan(Some(&source), &place(false))
        .err()
        .unwrap()
        .contains("native"));
    // A u16 master consumes two RGBA8-equivalent pixel units, regardless of the
    // byte fallback. 397 + 1 fallback + 2 master fits; 398 + 1 + 2 does not.
    let typed = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            1000,
            1000,
            vec![[1001, 2001, 3001, 65531]; 1_000_000],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    assert!(fill_layers(&source, 397)
        .source_plan_input(
            Some(SourceInput::Typed(&typed)),
            &place(true),
            24_000_000_000
        )
        .is_ok());
    assert!(fill_layers(&source, 398)
        .source_plan_input(
            Some(SourceInput::Typed(&typed)),
            &place(true),
            24_000_000_000
        )
        .is_err());
}
#[test]
fn replacement_counts_old_duplicate_revision_and_preserves_non_growing_oversized_edits() {
    let source = byte(1000, 1000);
    let placed = source
        .source_edit(
            Some(&source),
            &place(false),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mut duplicated = placed.duplicating_layer(1).unwrap();
    duplicated
        .layers
        .extend(vec![source.layers[0].clone(); 396]);
    // 399 fallback MP + 1 shared master MP is already at the 400 MP ceiling. A new
    // revision adds another master while the duplicate still needs the old one.
    let replace = SourceEdit::parse(r#"{"op":"replace","layer":1,"revision_id":"next"}"#).unwrap();
    assert!(duplicated
        .source_plan(Some(&source), &replace)
        .err()
        .unwrap()
        .contains("native"));
    // One old asset has no remaining reference after replacing its sole layer:
    // this changes revision identity, but not the serialized pixel total.
    let mut sole = placed.clone();
    sole.layers.extend(vec![source.layers[0].clone(); 397]);
    assert!(sole.source_plan(Some(&source), &replace).is_ok());
    // Existing oversized documents may still reduce their fallback storage.
    let mut oversized = placed;
    oversized.layers.extend(vec![source.layers[0].clone(); 399]);
    let shrink = SourceEdit::parse(
        r#"{"op":"transform","layer":1,"matrix":[0.5,0,0,0.5,0,0],"filter":"bilinear"}"#,
    )
    .unwrap();
    assert!(oversized.source_plan(None, &shrink).is_ok());
}
