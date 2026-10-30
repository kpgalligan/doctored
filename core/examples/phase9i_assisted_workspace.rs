//! Source-only case export for the Phase 9I assisted annotation workspace.
//!
//! Each named case is rebuilt through the SAME frozen recipe the evaluator
//! uses — `frozen::photo_source` for the composite an annotator sees and
//! `frozen::prior` for the starting mask — so the workspace bytes and the
//! scorer's inputs come from one implementation. The reference alpha is read
//! only to derive those two planes; truth is never written, named or hashed
//! here, and the output directory carries anonymous ids alone.
//!
//! `phase9i_assisted_workspace <jobs.json> <cases_dir> <crops_dir> <full_dir>`
//! with jobs `{"schema_version":1,"cases":[{"case","anonymous_id"}]}`. One
//! JSON line per case is printed: `{"anonymous_id","width","height"}`.
use image::{DynamicImage, GrayImage, Luma, RgbaImage};
use serde_json::{json, Value};
use std::path::Path;
#[allow(dead_code)]
#[path = "phase6_full_image_quality.rs"]
pub(crate) mod full;
use full::frozen;

/// `photo-<reference>-<background>-<start>` / `full-<id>-<background>-<start>`.
/// Reference ids contain hyphens, so the fixed fields are read from the ends.
pub(crate) fn parse_case(case: &str) -> (String, String, String, String) {
    let parts: Vec<&str> = case.split('-').collect();
    assert!(parts.len() >= 4, "unrecognized case id {case}");
    let corpus = parts[0].to_string();
    assert!(
        corpus == "photo" || corpus == "full",
        "unrecognized corpus in {case}"
    );
    let start = parts[parts.len() - 1].to_string();
    let background = parts[parts.len() - 2].to_string();
    assert!(
        matches!(start.as_str(), "blurred" | "binary"),
        "unrecognized start in {case}"
    );
    assert!(
        matches!(background.as_str(), "light" | "dark" | "checker"),
        "unrecognized background in {case}"
    );
    (
        corpus,
        parts[1..parts.len() - 2].join("-"),
        background,
        start,
    )
}

fn reference<'a>(manifest: &'a Value, id: &str, case: &str) -> &'a Value {
    manifest["references"]
        .as_array()
        .expect("reference array")
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no reference {id} for case {case}"))
}

/// The exact foreground+alpha the evaluator measures for this case: the pinned
/// 1280x1280 crop for the photographic corpus, the whole image for the full
/// corpus. Both assert the manifest's declared dimensions first.
pub(crate) fn case_foreground(case: &str, crops: &Path, full_dir: &Path) -> RgbaImage {
    let (corpus, id, _, _) = parse_case(case);
    if corpus == "photo" {
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(crops.join("references.json")).unwrap()).unwrap();
        assert_eq!(manifest["schema_version"], 1);
        let entry = reference(&manifest, &id, case);
        let name = entry["file"].as_str().unwrap();
        assert_eq!(
            Path::new(name).file_name().and_then(|value| value.to_str()),
            Some(name)
        );
        let image = image::open(crops.join(name)).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (2304, 3456));
        assert_eq!(entry["crop"], json!([512, 384, 1280, 1280]));
        return image::imageops::crop_imm(&image, 512, 384, 1280, 1280).to_image();
    }
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(full_dir.join("full-references.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    let entry = reference(&manifest, &id, case);
    let name = entry["file"].as_str().unwrap();
    assert_eq!(
        Path::new(name).file_name().and_then(|value| value.to_str()),
        Some(name)
    );
    let image = image::open(full_dir.join(name)).unwrap().to_rgba8();
    assert_eq!(
        entry["dimensions"],
        json!([image.width(), image.height()]),
        "declared dimensions for {case}"
    );
    image
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let jobs_path = Path::new(args.first().expect("jobs.json"));
    let cases_dir = Path::new(args.get(1).expect("case output directory"));
    let crops = Path::new(args.get(2).expect("verified crop reference directory"));
    let full_dir = Path::new(args.get(3).expect("verified full reference directory"));
    let jobs: Value = serde_json::from_slice(&std::fs::read(jobs_path).unwrap()).unwrap();
    assert_eq!(jobs["schema_version"], 1);
    std::fs::create_dir_all(cases_dir).unwrap();
    for job in jobs["cases"].as_array().expect("case array") {
        let case = job["case"].as_str().expect("case id");
        let anonymous = job["anonymous_id"].as_str().expect("anonymous id");
        assert!(
            anonymous
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
            "anonymous id must be lowercase hex-like: {anonymous}"
        );
        let directory = cases_dir.join(anonymous);
        assert!(!directory.exists(), "case directory already exists");
        let (_, _, background, start) = parse_case(case);
        let foreground = case_foreground(case, crops, full_dir);
        let (width, height) = foreground.dimensions();
        let truth = GrayImage::from_fn(width, height, |x, y| Luma([foreground.get_pixel(x, y)[3]]));
        let source = frozen::photo_source(&foreground, &background);
        let prior = frozen::prior(&truth, &start);
        std::fs::create_dir(&directory).unwrap();
        DynamicImage::ImageRgba8(source)
            .to_rgb8()
            .save(directory.join("source.png"))
            .unwrap();
        prior.save(directory.join("prior.png")).unwrap();
        println!(
            "{}",
            json!({"anonymous_id":anonymous,"width":width,"height":height})
        );
    }
}
