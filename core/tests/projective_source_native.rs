//! RSPJ's conditional wrapper keeps a projective source bound to its original
//! embedded master. Corrupt table metadata is refused before pixel decoding.

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use doctored_core::source_transform::SourceTransform;
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn request() -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[1],
        "source_rect":[0,0,4,4],"quad":[0,0,20.0/7.0,0,20.0/7.0,20.0/7.0,0,4],
        "filter":"bilinear"})
        .to_string(),
    )
    .unwrap()
}

fn source_place() -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"place","source_id":"projective-master",
        "revision_id":"first","name":"Source","x":0,"y":0})
        .to_string(),
    )
    .unwrap()
}

fn byte() -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([3, 5, 7, 255])));
    let master = DrDocument::from_pixels(RgbaImage::from_fn(4, 4, |x, y| {
        Rgba([
            (x * 43 + 1) as u8,
            (y * 37 + 3) as u8,
            199,
            if x == 0 { 0 } else { 255 },
        ])
    }));
    base.source_edit(Some(&master), &source_place(), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn typed() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(8, 8, vec![[1, 3, 5, 65535]; 64], profile.clone(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let master = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            4,
            4,
            (0..16)
                .map(|v| {
                    [
                        v * 97 + 1,
                        v * 101 + 3,
                        65535 - v * 59,
                        if v % 4 == 0 { 0 } else { 32769 },
                    ]
                })
                .collect(),
            profile,
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    base.source_edit(Some(&master), &source_place(), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn both_depths_roundtrip_projective_master_and_restore_affine_legacy_output() {
    let dir = TempDir::new().unwrap();
    let old_byte = byte();
    let byte = old_byte
        .geometry_edit(&request(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let source = byte.layers[1].effective_source(&byte.profile).unwrap();
    assert!(matches!(source.transform, SourceTransform::Projective(_)));
    assert!(Arc::ptr_eq(
        &source.asset,
        &old_byte.layers[1].retained_source.as_ref().unwrap().asset
    ));
    let path = dir.path().join("byte.rz");
    byte.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45_u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &51_u32.to_le_bytes());
    assert!(bytes.windows(4).any(|window| window == b"RSPJ"));
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap()["transform_kind"],
        "projective"
    );
    reopened.save_native(path.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let rasterized = reopened
        .source_edit(
            None,
            &SourceEdit::parse(&json!({"op":"rasterize","layer":1}).to_string()).unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    rasterized.save_native(path.to_str().unwrap()).unwrap();
    let no_table = std::fs::read(&path).unwrap();
    assert!(!no_table.windows(4).any(|window| window == b"RSPJ"));

    let old_typed = typed();
    let typed = old_typed
        .geometry(&request(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let source = typed.layer(1).unwrap().effective_source().unwrap();
    assert!(matches!(source.transform, SourceTransform::Projective(_)));
    assert!(Arc::ptr_eq(
        &source.asset,
        &old_typed
            .layer(1)
            .unwrap()
            .effective_source()
            .unwrap()
            .asset
    ));
    let path = dir.path().join("typed.rz");
    typed
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46_u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &52_u32.to_le_bytes());
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap()["transform_kind"],
        "projective"
    );
    reopened
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn malformed_projective_tables_refuse_before_any_reopen() {
    let dir = TempDir::new().unwrap();
    let projective = byte()
        .geometry_edit(&request(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("bad.rz");
    projective.save_native(path.to_str().unwrap()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let at = original
        .windows(4)
        .position(|window| window == b"RSPJ")
        .unwrap();
    for mutation in 0..4 {
        let mut bytes = original.clone();
        match mutation {
            0 => bytes[at] = b'X',
            1 => bytes[at + 4] = 2,
            2 => bytes[at + 8..at + 12].copy_from_slice(&0_u32.to_le_bytes()),
            _ => bytes[at + 16..at + 24].copy_from_slice(&f64::NAN.to_le_bytes()),
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(
            DrDocument::open(path.to_str().unwrap()).is_err(),
            "mutation {mutation}"
        );
    }
}

fn projective_table(bytes: &[u8]) -> usize {
    bytes
        .windows(4)
        .position(|window| window == b"RSPJ")
        .unwrap()
}

fn set_matrix(bytes: &mut [u8], at: usize, matrix: [f64; 9]) {
    for (index, coefficient) in matrix.iter().enumerate() {
        bytes[at + 16 + index * 8..at + 24 + index * 8].copy_from_slice(&coefficient.to_le_bytes());
    }
}

fn reject_bad_table(mut bytes: Vec<u8>, typed: bool, case: &str, path: &std::path::Path) {
    // Keep the PNG header inspectable but damage its compressed payload. If
    // the table check were skipped, full fallback decode would fail instead.
    let png = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .unwrap();
    bytes[png + 4] ^= 0x7f;
    std::fs::write(path, bytes).unwrap();
    let error = if typed {
        match PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None) {
            Err(error) => error,
            Ok(_) => panic!("typed malformed {case} reopened"),
        }
    } else {
        match DrDocument::open(path.to_str().unwrap()) {
            Err(error) => error,
            Ok(_) => panic!("byte malformed {case} reopened"),
        }
    };
    let shown = error.replace(&path.to_string_lossy().into_owned(), "<path>");
    assert!(
        !shown.to_lowercase().contains("png") && !shown.contains("decode"),
        "{case} reached fallback decode before table refusal: {error}"
    );
}

#[test]
fn corrupt_projective_table_corpus_refuses_before_fallback_at_both_depths() {
    let dir = TempDir::new().unwrap();
    for typed in [false, true] {
        let path = dir.path().join(if typed {
            "typed-corrupt.rz"
        } else {
            "byte-corrupt.rz"
        });
        if typed {
            typed_document_for_corruption()
                .save_native(path.to_str().unwrap(), LIMIT, 0, None)
                .unwrap();
        } else {
            byte()
                .geometry_edit(&request(), LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .save_native(path.to_str().unwrap())
                .unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let at = projective_table(&original);
        let length = 12 + 76;
        let cases: Vec<(&str, Vec<u8>)> = {
            let mut result = Vec::new();
            macro_rules! mutate {
                ($name:expr, $change:expr) => {{
                    let mut bytes = original.clone();
                    ($change)(&mut bytes);
                    result.push(($name, bytes));
                }};
            }
            mutate!("bad tag", |bytes: &mut Vec<u8>| bytes[at] = b'X');
            mutate!("bad schema", |bytes: &mut Vec<u8>| bytes[at + 4..at + 8]
                .copy_from_slice(&2_u32.to_le_bytes()));
            mutate!("zero count", |bytes: &mut Vec<u8>| bytes[at + 8..at + 12]
                .copy_from_slice(&0_u32.to_le_bytes()));
            mutate!("extra count", |bytes: &mut Vec<u8>| bytes[at + 8..at + 12]
                .copy_from_slice(&2_u32.to_le_bytes()));
            mutate!("out of range target", |bytes: &mut Vec<u8>| bytes
                [at + 12..at + 16]
                .copy_from_slice(&u32::MAX.to_le_bytes()));
            mutate!("non-source target", |bytes: &mut Vec<u8>| bytes
                [at + 12..at + 16]
                .copy_from_slice(&0_u32.to_le_bytes()));
            mutate!("nonfinite map", |bytes: &mut Vec<u8>| bytes
                [at + 16..at + 24]
                .copy_from_slice(&f64::NAN.to_le_bytes()));
            mutate!("singular map", |bytes: &mut Vec<u8>| set_matrix(
                bytes,
                at,
                [1., 0., 0., 0., 0., 0., 0., 0., 1.]
            ));
            mutate!("horizon map", |bytes: &mut Vec<u8>| set_matrix(
                bytes,
                at,
                [1., 0., 0., 0., 1., 0., -0.5, 0., 1.]
            ));
            mutate!("wrong extent", |bytes: &mut Vec<u8>| set_matrix(
                bytes,
                at,
                [2., 0., 0., 0., 2., 0., 0., 0., 1.]
            ));
            mutate!("missing table", |bytes: &mut Vec<u8>| {
                bytes.drain(at..at + length);
            });
            mutate!("extra table", |bytes: &mut Vec<u8>| {
                let table = bytes[at..at + length].to_vec();
                bytes.splice(at + length..at + length, table);
            });
            mutate!("truncated table", |bytes: &mut Vec<u8>| bytes
                .truncate(at + length - 1));
            mutate!("trailing bytes", |bytes: &mut Vec<u8>| bytes
                .extend_from_slice(b"TAIL"));
            result
        };
        for (case, bytes) in cases {
            // Truncation discards fallback PNG as well; every other case has
            // an intact payload whose signature is poisoned by this helper.
            if case == "truncated table" {
                std::fs::write(&path, bytes).unwrap();
                let refused = if typed {
                    PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).is_err()
                } else {
                    DrDocument::open(path.to_str().unwrap()).is_err()
                };
                assert!(refused, "{case} at depth {}", if typed { 16 } else { 8 });
            } else {
                reject_bad_table(bytes, typed, case, &path);
            }
        }
    }
}

fn typed_document_for_corruption() -> PrecisionDocument {
    typed()
        .geometry(&request(), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn duplicate_and_unsorted_projective_targets_refuse_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let second_place = SourceEdit::parse(
        &json!({"op":"place","source_id":"second-master",
        "revision_id":"first","name":"Second","x":0,"y":0})
        .to_string(),
    )
    .unwrap();
    let second_edit = GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[2],
        "source_rect":[0,0,4,4],
        "quad":[0,0,20.0/7.0,0,20.0/7.0,20.0/7.0,0,4],
        "filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    for high_depth in [false, true] {
        let path = dir.path().join(if high_depth {
            "typed-two.rz"
        } else {
            "byte-two.rz"
        });
        if high_depth {
            let first = typed_document_for_corruption();
            let master = PrecisionDocument::from_image(
                PrecisionImage::from_rgba16(
                    4,
                    4,
                    vec![[13, 25, 37, 65535]; 16],
                    IccProfile::srgb(),
                    None,
                )
                .unwrap(),
                Metadata::default(),
                Resolution::default(),
            )
            .unwrap();
            let second = first
                .source_edit(Some(&master), &second_place, LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .geometry(&second_edit, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            second
                .save_native(path.to_str().unwrap(), LIMIT, 0, None)
                .unwrap();
        } else {
            let first = byte()
                .geometry_edit(&request(), LIMIT, 0, None)
                .unwrap()
                .unwrap();
            let master =
                DrDocument::from_pixels(RgbaImage::from_pixel(4, 4, Rgba([13, 25, 37, 255])));
            let second = first
                .source_edit(Some(&master), &second_place, LIMIT, 0, None)
                .unwrap()
                .unwrap()
                .geometry_edit(&second_edit, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            second.save_native(path.to_str().unwrap()).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let at = projective_table(&original);
        assert_eq!(
            u32::from_le_bytes(original[at + 8..at + 12].try_into().unwrap()),
            2
        );
        let second = at + 12 + 76;
        for (name, first_target, second_target) in
            [("duplicate", 1_u32, 1_u32), ("unsorted", 2_u32, 1_u32)]
        {
            let mut bytes = original.clone();
            bytes[at + 12..at + 16].copy_from_slice(&first_target.to_le_bytes());
            bytes[second..second + 4].copy_from_slice(&second_target.to_le_bytes());
            reject_bad_table(bytes, high_depth, name, &path);
        }
    }
}
