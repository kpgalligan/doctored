//! Import geometry/storage are checked on the PNG header before any pixels
//! decode, including files which contain no pixel stream at all.
use image::{ColorType, GrayImage, RgbImage};
use std::{fs, path::Path};
#[allow(dead_code)]
#[path = "../examples/phase6_external_matte_quality.rs"]
mod adapter;

#[test]
fn native_planes_preserve_every_byte_and_reject_conversion() {
    let directory = tempfile::tempdir().unwrap();
    let gray = GrayImage::from_raw(3, 2, vec![0, 1, 127, 128, 254, 255]).unwrap();
    let rgb = RgbImage::from_raw(3, 2, (0..18).map(|v| v * 13).collect()).unwrap();
    let gray_path = directory.path().join("alpha.png");
    let rgb_path = directory.path().join("source.png");
    gray.save(&gray_path).unwrap();
    rgb.save(&rgb_path).unwrap();
    for (path, color, bytes) in [
        (&gray_path, ColorType::L8, gray.as_raw()),
        (&rgb_path, ColorType::Rgb8, rgb.as_raw()),
    ] {
        let decoded = adapter::decode_native_png(path, (3, 2), color).unwrap();
        assert_eq!(decoded.as_bytes(), bytes);
        assert_eq!(decoded.color(), color);
    }
    assert!(adapter::decode_native_png(&rgb_path, (3, 2), ColorType::L8)
        .unwrap_err()
        .contains("native L8"));
    assert!(
        adapter::decode_native_png(&gray_path, (3, 2), ColorType::Rgb8)
            .unwrap_err()
            .contains("native Rgb8")
    );
}

#[test]
fn header_only_wrong_geometry_or_storage_refuses_before_decode() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("plane.png");
    GrayImage::new(3, 2).save(&path).unwrap();
    let valid_header = fs::read(&path).unwrap()[..33].to_vec();
    for (index, value) in [(24, 1), (24, 16), (25, 3), (25, 6)] {
        // Invalid CRC and absent IDAT are intentional: the native storage
        // refusal must happen before attempting a decoder or pixel stream.
        let mut header = valid_header.clone();
        header[index] = value;
        fs::write(&path, header).unwrap();
        assert!(adapter::decode_native_png(&path, (3, 2), ColorType::L8)
            .unwrap_err()
            .contains("native L8"));
    }
    let mut huge = valid_header.clone();
    huge[16..20].copy_from_slice(&100_000_u32.to_be_bytes());
    huge[20..24].copy_from_slice(&100_000_u32.to_be_bytes());
    fs::write(&path, huge).unwrap();
    assert!(adapter::decode_native_png(&path, (3, 2), ColorType::L8)
        .unwrap_err()
        .contains("dimensions"));
    fs::write(&path, valid_header).unwrap();
    assert!(adapter::decode_native_png(&path, (3, 2), ColorType::L8)
        .unwrap_err()
        .contains("failed to inspect"));
}

#[test]
fn artifact_paths_reject_repository_and_symlinked_parents() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert!(adapter::outside_repository(repository).is_err());
    assert!(adapter::outside_repository(&repository.join("Cargo-artifact-never-created")).is_err());
    let external = directory.path().join("new-report");
    assert_eq!(
        adapter::outside_repository(&external).unwrap(),
        directory.path().canonicalize().unwrap().join("new-report")
    );
    assert!(!external.exists());
    assert!(adapter::outside_repository(&external.join("missing-parent")).is_err());
    #[cfg(unix)]
    {
        let link = directory.path().join("repository-link");
        std::os::unix::fs::symlink(repository, &link).unwrap();
        assert!(adapter::outside_repository(&link.join("never-created")).is_err());
        assert!(adapter::outside_repository(&link.join("core/Cargo.toml")).is_err());
    }
}
