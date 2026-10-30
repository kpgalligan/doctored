//! Full native-size photographic reference qualification. This adapter reuses
//! the frozen metrics/candidate settings but neither crops nor resizes inputs.
//! A fixed2GiB harness envelope refuses before decoding/allocating full images;
//! kernel unknown/boundary limits remain separate explicit refusals.
use image::{GrayImage, Luma};
use serde_json::{json, Value};
use std::path::Path;
#[allow(dead_code)]
#[path = "phase6_matting_quality.rs"]
pub(crate) mod frozen;
use frozen::quality;

const HARNESS_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const HARNESS_FIXED: u64 = 64 * 1024 * 1024;

pub(crate) fn run(
    root: &Path,
    references: &Path,
    candidate: frozen::Candidate,
    working_limit: u64,
) {
    assert!(
        (HARNESS_FIXED..=HARNESS_LIMIT).contains(&working_limit),
        "working limit must be64MiB..2GiB"
    );
    std::fs::create_dir_all(root).unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(references.join("full-references.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    let mut records = Vec::new();
    for reference in manifest["references"].as_array().expect("reference list") {
        let id = reference["id"].as_str().unwrap();
        let name = reference["file"].as_str().unwrap();
        assert_eq!(
            Path::new(name).file_name().and_then(|v| v.to_str()),
            Some(name)
        );
        let path = references.join(name);
        let (width, height) = image::image_dimensions(&path).unwrap();
        assert_eq!(reference["dimensions"], json!([width, height]));
        let pixels = u64::from(width) * u64::from(height);
        //128N foreground scratch plus borrowed/decoded source, float colors,
        //alpha/prior/trimap, result planes and exported comparison canvases.
        //256N+64MiB conservatively encloses their overlapping lifetimes.
        // Joint F adds12N, already inside this original allowance: worst oracle
        // overlap49N caller +8N FFI copies +128N estimator +4N result copy +12N
        // metric temporary =201N+1MiB. Joint candidate overlap is at most
        //36N caller +64N+16MiB scratch. No admission limit is raised.
        let reservation = pixels
            .checked_mul(256)
            .and_then(|v| v.checked_add(HARNESS_FIXED));
        if reservation.is_none_or(|v| v > working_limit) {
            for background in ["light", "dark", "checker"] {
                for start in ["blurred", "binary"] {
                    let mut refusal = json!({"case":format!("full-{id}-{background}-{start}"),"reference":reference,"dimensions":[width,height],"evaluation_extent":"full native image","refusal":format!("full-image qualification exceeds{}MiB retained-plus-scratch envelope; no crop or resize substituted", working_limit / (1024*1024)),"estimated_working_bytes":reservation,"passes_analytic_gate":false,"passes_end_to_end_gate":false,"sampling":{"refusal":"harness memory admission"},"oracle_alpha_color":{"passes_color_gate":false,"refusal":"harness memory admission"},"sampled_alpha_color":{"passes_color_gate":false,"refusal":"harness memory admission"}});
                    if candidate.name != "sampled" {
                        let fields = refusal.as_object_mut().unwrap();
                        let metadata = fields.remove("sampling").unwrap();
                        let color = fields.remove("sampled_alpha_color").unwrap();
                        fields.insert("alpha_candidate".into(), metadata);
                        fields.insert("estimated_alpha_color".into(), color);
                    }
                    records.push(refusal);
                }
            }
            continue;
        }
        let foreground = image::open(path).unwrap().to_rgba8();
        let truth = GrayImage::from_fn(width, height, |x, y| Luma([foreground.get_pixel(x, y)[3]]));
        let colors = quality::colors(&foreground);
        for background in ["light", "dark", "checker"] {
            let source = frozen::photo_source(&foreground, background);
            for start in ["blurred", "binary"] {
                let name = format!("full-{id}-{background}-{start}");
                let mut result = frozen::evaluate_candidate(
                    (root, &name),
                    &source,
                    &truth,
                    &colors,
                    start,
                    background == "light" && start == "blurred",
                    candidate,
                );
                if result["has_fractional_reference"] != true {
                    result["passes_analytic_gate"] = json!(false);
                }
                result["reference"] = reference.clone();
                result["evaluation_extent"] = json!("full native image; no crop or resize");
                result["estimated_working_bytes"] = json!(reservation);
                records.push(result);
            }
        }
    }
    let report = json!({"schema_version":1,"solver":candidate.name,"domain":"encoded RGB; controlled recompositions of supplied F+alpha","harness_limit_bytes":working_limit,"harness_estimate":"256*full_image_pixels+64MiB","color_gate":frozen::color_gate_contract(),"full_image_summary":frozen::summary_for(&records,candidate),"physical_translucency_reference_available":false,"reference_manifest":manifest,"full_images":records});
    std::fs::write(
        root.join("metrics.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::copy(
        references.join("ATTRIBUTION.md"),
        root.join("ATTRIBUTION.md"),
    )
    .unwrap();
    println!("full images {}", report["full_image_summary"]);
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let limit = args.get(3).map_or(HARNESS_LIMIT, |value| {
        value.parse().expect("working limit in bytes")
    });
    run(
        Path::new(args.get(1).expect("output directory")),
        Path::new(args.get(2).expect("verified full reference directory")),
        frozen::SAMPLED,
        limit,
    );
}
