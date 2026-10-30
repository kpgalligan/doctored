use crate::raw_geometry_fixture as fixture;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::retained_source::SourceEdit;
use fixture::{image, LIMIT};
use serde_json::json;

fn edit(text: serde_json::Value) -> SourceEdit {
    SourceEdit::parse(&text.to_string()).unwrap()
}

#[test]
fn replacement_precomposes_affine_and_native_roundtrips() {
    let old_master = image(7, 5);
    let new_master = image(5, 7);
    let canvas = image(15, 15);
    let placed = canvas
        .source_edit(
            Some(&old_master),
            &edit(json!({
                "op":"place","source_id":"raw-coordinate-native","name":"RAW","x":0,"y":0
            })),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let h = [1.0, 0.15, 0.05, 1.0, 3.0, 4.0];
    let transformed = placed
        .source_edit(
            None,
            &edit(json!({
                "op":"transform","layer":1,"matrix":h,"filter":"bilinear"
            })),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let m = [0.0, -1.0, 1.0, 0.0, 0.0, 5.0];
    let replaced = transformed
        .source_edit(
            Some(&new_master),
            &edit(json!({
                "op":"replace","layer":1,"revision_id":"raw-geometry-next",
                "source_coordinate_map":m
            })),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let actual: Vec<f64> =
        serde_json::from_value(replaced.source_info(1).unwrap()["transform"].clone()).unwrap();
    let expected = [
        h[0] * m[0] + h[2] * m[1],
        h[1] * m[0] + h[3] * m[1],
        h[0] * m[2] + h[2] * m[3],
        h[1] * m[2] + h[3] * m[3],
        h[0] * m[4] + h[2] * m[5] + h[4],
        h[1] * m[4] + h[3] * m[5] + h[5],
    ];
    assert_eq!(actual, expected);
    assert_eq!(
        replaced.source_info(1).unwrap()["source_id"],
        "raw-coordinate-native"
    );
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("first.rz");
    let second = temporary.path().join("second.rz");
    replaced
        .save_native(first.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened = doctored_core::precision_document::PrecisionDocument::open_native(
        first.to_str().unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap(),
        replaced.source_info(1).unwrap()
    );
    reopened
        .save_native(second.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap()
    );
    println!("raw-source-binding affine_precomposition=exact native_reopen_resave=byte_identical");
}

#[test]
fn replacement_rejects_mutually_exclusive_matrices() {
    assert!(SourceEdit::parse(
        &json!({"op":"replace","layer":0,"revision_id":"r2",
        "source_transform":[1,0,0,1,0,0],
        "source_coordinate_map":[1,0,0,1,0,0]})
        .to_string()
    )
    .is_err());
}

#[test]
fn replacement_precomposes_existing_projective_binding() {
    let old_master = image(7, 5);
    let new_master = image(5, 7);
    let placed = image(24, 24)
        .source_edit(
            Some(&old_master),
            &edit(json!({"op":"place","source_id":"raw-projective","name":"RAW","x":0,"y":0})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let perspective = GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[1],"source_rect":[0,0,7,5],
            "quad":[1,1,9,1,8,8,0,6],"filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    let projected = placed
        .geometry(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let prior: Vec<f64> =
        serde_json::from_value(projected.source_info(1).unwrap()["projective_transform"].clone())
            .unwrap();
    assert_eq!(prior.len(), 9);
    let m = [0.0, 1.0, 0.0, -1.0, 0.0, 5.0, 0.0, 0.0, 1.0];
    let replaced = projected
        .source_edit(
            Some(&new_master),
            &edit(
                json!({"op":"replace","layer":1,"revision_id":"projective-next",
                "source_coordinate_map":[0,-1,1,0,0,5]}),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let info = replaced.source_info(1).unwrap();
    assert_eq!(info["transform_kind"], "projective");
    let actual: Vec<f64> = serde_json::from_value(info["projective_transform"].clone()).unwrap();
    let mut expected = [0.0; 9];
    for row in 0..3 {
        for col in 0..3 {
            expected[row * 3 + col] = (0..3).map(|i| prior[row * 3 + i] * m[i * 3 + col]).sum();
        }
    }
    let scale = expected[8];
    for (got, want) in actual.iter().zip(expected.iter()) {
        assert!((got - want / scale).abs() < 1e-12);
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("projective.rz");
    replaced
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened = doctored_core::precision_document::PrecisionDocument::open_native(
        path.to_str().unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap();
    assert_eq!(reopened.source_info(1).unwrap(), info);
    println!("raw-source-binding projective_precomposition=exact native_reopen=exact");
}
