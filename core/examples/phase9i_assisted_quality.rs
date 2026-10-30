//! Phase 9I assisted scoring: one exported workspace plus its human replays,
//! scored in exactly the automatic runs' report shape.
//!
//! `phase9i_assisted_quality <workspace> <mapping.json> <crops> <full> <output>
//! [--synthetic-dry-run]`
//!
//! Per case the label plane is rebuilt from the replay AS THE APP RENDERED IT —
//! every per-event hash is reproduced through `assisted_replay::label_plane`
//! before the last one is used — and handed to ONE frozen kernel as hard
//! `user_constraints` over the workspace's own source and starting prior. The
//! reference foreground is read for METRICS ONLY, exactly as every existing
//! evaluator reads it, and only after the workspace's source and prior have
//! been proved byte-for-byte equal to the frozen recipe's own output.
//!
//! `--synthetic-dry-run` accepts a synthetic replay so the plumbing can be
//! exercised end to end. Such a run is a pipeline exercise and NEVER an
//! assisted result: the report says so in `assisted_result_claimable`.
use doctored_core::{
    assisted_replay, closed_form_matting::ClosedFormParams, matting_input::SoftPriorInput,
};
use image::{DynamicImage, GrayImage, Luma};
use serde_json::{json, Value};
use std::path::Path;
#[allow(dead_code)]
#[path = "phase9i_assisted_workspace.rs"]
mod workspace;
use workspace::full::frozen;
#[path = "../tests/common/assisted_scoring.rs"]
mod scoring;
use scoring::ReplayInput;

const HARNESS_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const HARNESS_FIXED: u64 = 64 * 1024 * 1024;

/// Owner decision 2: V2's tiled kernel, parameters unchanged, no protection.
/// The global solver refuses every subset extent at its 500,000-unknown cap,
/// this one ran all 90 cases inside the 2 GiB harness, and
/// `closed_form_matting::estimate_input_job` copies labeled pixels verbatim, so
/// the human labels are exact rather than merely weighted.
fn candidate(
    _case: &str,
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    user_constraints: &[u8],
    prior: &[u8],
) -> Result<frozen::AlphaResult, String> {
    let params = ClosedFormParams::default();
    let input = SoftPriorInput {
        version: 1,
        user_constraints: Some(user_constraints),
        prior_weight: 1.0,
    };
    let result = doctored_core::tiled_soft_matting::estimate(
        colors,
        (width, height),
        prior,
        input,
        &params,
        None,
    )?;
    Ok(frozen::AlphaResult {
        foreground: None,
        diagnostics: json!({"method":"shared closed-form local tile objectives with version1 soft automatic prior",
            "input_policy_version":1,"prior_weight":input.prior_weight,
            "user_constraints":"assisted_human_labels_v1",
            "tile_output_side":256,"tile_halo":16,"tile_count":result.tile_count,
            "processed_tile_pixels":result.processed_tile_pixels,
            "iteration_limit":result.iteration_limit,"iterations_total":result.iterations_total,
            "maximum_tile_residual_rms":result.maximum_tile_residual_rms,
            "estimated_stencil_terms":result.estimated_stencil_terms,
            "all_local_systems_converged":true,"global_system_convergence_claimed":false,
            "identifiability_confidence_available":false,
            "parameters":{"epsilon":params.epsilon,"max_iterations_before_work_admission":params.max_iterations,
                "residual_rms_tolerance":params.residual_rms_tolerance}}),
        alpha: result.alpha,
        confidence: None,
    })
}

const ASSISTED: frozen::Candidate = frozen::Candidate {
    protect_foreground: false,
    input_policy: frozen::InputPolicy::AssistedHumanLabelsV1,
    name: "assisted_tiled_soft_prior_v1",
    estimate: candidate,
};

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn reference_entry<'a>(manifest: &'a Value, id: &str) -> &'a Value {
    manifest["references"]
        .as_array()
        .expect("reference array")
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no reference {id}"))
}

fn relative(files: &[std::path::PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dry_run = args.iter().any(|value| value == "--synthetic-dry-run");
    let positional: Vec<&String> = args
        .iter()
        .filter(|value| !value.starts_with("--"))
        .collect();
    assert_eq!(
        positional.len(),
        5,
        "phase9i_assisted_quality <workspace> <mapping.json> <crops> <full> <output> [--synthetic-dry-run]"
    );
    let workspace_dir = Path::new(positional[0]);
    let mapping_path = Path::new(positional[1]);
    let crops = Path::new(positional[2]);
    let full_dir = Path::new(positional[3]);
    let output = Path::new(positional[4]);

    let manifest = read_json(&workspace_dir.join("manifest.json"));
    assert_eq!(manifest["workspace_schema_version"], 1);
    let workspace_id = manifest["workspace_id"].as_str().expect("workspace_id");
    let task_card_sha256 = manifest["task_card_sha256"]
        .as_str()
        .expect("task_card_sha256");
    let manifest_cases = manifest["cases"].as_array().expect("manifest case array");

    let mapping = read_json(mapping_path);
    assert_eq!(mapping["mapping_schema_version"], 1);
    assert_eq!(
        mapping["workspace_id"].as_str(),
        Some(workspace_id),
        "the mapping belongs to another workspace"
    );
    let mut mapping_cases: Vec<&Value> = mapping["cases"]
        .as_array()
        .expect("mapping case array")
        .iter()
        .collect();
    mapping_cases.sort_by_key(|entry| entry["case"].as_str().unwrap_or_default().to_string());
    let mut manifest_ids: Vec<&str> = manifest_cases
        .iter()
        .map(|entry| entry["anonymous_id"].as_str().expect("anonymous_id"))
        .collect();
    let mut mapping_ids: Vec<&str> = mapping_cases
        .iter()
        .map(|entry| entry["anonymous_id"].as_str().expect("anonymous_id"))
        .collect();
    manifest_ids.sort_unstable();
    mapping_ids.sort_unstable();
    assert_eq!(
        manifest_ids, mapping_ids,
        "the manifest and mapping describe different cases"
    );

    // Reference provenance, carried into the report exactly as the automatic
    // runs carry it. Neither manifest ever builds a solver input.
    let crop_references = read_json(&crops.join("references.json"));
    assert_eq!(crop_references["schema_version"], 1);
    let full_references = read_json(&full_dir.join("full-references.json"));
    assert_eq!(full_references["schema_version"], 1);

    std::fs::create_dir_all(output).unwrap();
    let mut crop_records = Vec::new();
    let mut full_records = Vec::new();
    let mut refused = Vec::new();
    let mut exposed = Vec::new();

    for entry in &mapping_cases {
        let case = entry["case"].as_str().expect("case id");
        let anonymous = entry["anonymous_id"].as_str().expect("anonymous_id");
        let corpus = entry["corpus"].as_str().expect("corpus");
        let reference_id = entry["reference_id"].as_str().expect("reference_id");
        let background = entry["background"].as_str().expect("background");
        let start = entry["start"].as_str().expect("start");
        let exposed_declared = entry["annotator_exposed"] == json!(true);
        assert_eq!(
            workspace::parse_case(case),
            (
                corpus.to_string(),
                reference_id.to_string(),
                background.to_string(),
                start.to_string()
            ),
            "the mapping fields disagree with the case id"
        );
        let case_entry = manifest_cases
            .iter()
            .find(|value| value["anonymous_id"].as_str() == Some(anonymous))
            .expect("manifest case");
        let inputs_case = scoring::case_inputs(case_entry)
            .unwrap_or_else(|e| panic!("manifest case {anonymous}: {e}"));
        let inputs = scoring::solver_inputs(workspace_dir, &inputs_case)
            .unwrap_or_else(|e| panic!("workspace case {anonymous}: {e}"));

        // METRICS ONLY, and never before the workspace planes are proved to be
        // this case's: a mismatch means the wrong corpus, which is a panic.
        let foreground = workspace::case_foreground(case, crops, full_dir);
        let (width, height) = foreground.dimensions();
        let truth = GrayImage::from_fn(width, height, |x, y| Luma([foreground.get_pixel(x, y)[3]]));
        assert_eq!(
            DynamicImage::ImageRgba8(frozen::photo_source(&foreground, background)).to_rgb8(),
            inputs.source,
            "the workspace source for {case} is not the frozen recipe's own"
        );
        assert_eq!(
            frozen::prior(&truth, start),
            inputs.prior,
            "the workspace prior for {case} is not the frozen recipe's own"
        );
        let truth_f = frozen::quality::colors(&foreground);

        let pixels = u64::from(width) * u64::from(height);
        let reservation = pixels
            .checked_mul(256)
            .and_then(|value| value.checked_add(HARNESS_FIXED));

        let mut refusal: Option<String> = None;
        let mut labels: Option<GrayImage> = None;
        let mut audited = 0usize;
        if reservation.is_none_or(|value| value > HARNESS_LIMIT) {
            refusal = Some(format!(
                "full-image qualification exceeds {}MiB retained-plus-scratch envelope; no crop or resize substituted",
                HARNESS_LIMIT / (1024 * 1024)
            ));
        } else {
            match &inputs.replay {
                ReplayInput::Missing => refusal = Some("no replay".into()),
                ReplayInput::Malformed { error, .. } => {
                    refusal = Some(format!("malformed replay: {error}"));
                }
                ReplayInput::Parsed {
                    replay, summary, ..
                } => match scoring::check_scoreable(
                    replay,
                    summary,
                    &inputs_case,
                    workspace_id,
                    task_card_sha256,
                    dry_run,
                ) {
                    Err(reason) => refusal = Some(format!("replay refused: {reason}")),
                    Ok(()) => match scoring::verify_replay_hashes(replay) {
                        Err(error) => {
                            refusal = Some(format!("replay hash audit failed: {error}"));
                        }
                        Ok(events) => {
                            audited = events;
                            let plane = assisted_replay::label_plane(replay, width, height, None)
                                .expect("the audited replay rasterizes");
                            labels = Some(
                                GrayImage::from_raw(width, height, plane).expect("label extent"),
                            );
                        }
                    },
                },
            }
        }

        let mut record = match &labels {
            Some(labels) => {
                labels
                    .save(output.join(format!("{case}-labels.png")))
                    .unwrap();
                let source = DynamicImage::ImageRgb8(inputs.source.clone()).to_rgba8();
                frozen::evaluate_candidate_with_labels(
                    (output, case),
                    &source,
                    &truth,
                    &truth_f,
                    start,
                    true,
                    ASSISTED,
                    Some(labels),
                )
            }
            None => scoring::refusal_record(
                case,
                (width, height),
                frozen::quality::alpha_metrics(&truth, &inputs.prior),
                truth.as_raw().iter().any(|&a| a > 0 && a < 255),
                ASSISTED.name,
                refusal.as_deref().expect("a refusal has a reason"),
            ),
        };

        let (reference, reference_files) = if corpus == "photo" {
            let value = reference_entry(&crop_references, reference_id);
            let file = value["file"].as_str().unwrap_or_default().to_string();
            (value.clone(), vec!["references.json".to_string(), file])
        } else {
            let value = reference_entry(&full_references, reference_id);
            let file = value["file"].as_str().unwrap_or_default().to_string();
            (
                value.clone(),
                vec!["full-references.json".to_string(), file],
            )
        };
        record["reference"] = reference;
        if corpus == "full" {
            record["evaluation_extent"] = json!("full native image; no crop or resize");
            record["estimated_working_bytes"] = json!(reservation);
        }
        let mut assisted = json!({"anonymous_id":anonymous,"annotator_id":Value::Null,
            "origin":Value::Null,"outcome":Value::Null,"modifying_gestures":Value::Null,
            "elapsed_ms":Value::Null,"active_strokes":Value::Null,
            "annotator_exposed_declared":exposed_declared,
            "annotator_reports_prior_exposure":Value::Null,"replay_sha256":Value::Null,
            "label_plane_sha256":Value::Null,
            "replay_hash_audit":{"events":audited,"prefix_planes_verified":audited},
            "solver_input_files":relative(&inputs.read_files),
            "reference_files_metrics_only":reference_files,
            "refusal":refusal});
        let mut reports_exposure = false;
        match &inputs.replay {
            ReplayInput::Missing => {}
            ReplayInput::Malformed { sha256, .. } => assisted["replay_sha256"] = json!(sha256),
            ReplayInput::Parsed {
                sha256,
                replay,
                summary,
            } => {
                reports_exposure = replay.annotator_reports_prior_exposure == Some(true);
                assisted["annotator_id"] = json!(replay.annotator_id);
                assisted["origin"] = json!(replay.origin);
                assisted["outcome"] = json!(replay.outcome);
                assisted["modifying_gestures"] = json!(summary.modifying_gestures);
                assisted["elapsed_ms"] = json!(summary.elapsed_ms);
                assisted["active_strokes"] = json!(summary.active_strokes);
                assisted["annotator_reports_prior_exposure"] =
                    json!(replay.annotator_reports_prior_exposure);
                assisted["replay_sha256"] = json!(sha256);
                assisted["label_plane_sha256"] = json!(replay
                    .label_plane
                    .as_ref()
                    .map(|plane| plane.sha256.clone()));
            }
        }
        record["annotator_exposed"] = json!(exposed_declared || reports_exposure);
        record["assisted"] = assisted;
        if exposed_declared || reports_exposure {
            exposed.push(json!(case));
        }
        if let Some(reason) = &refusal {
            refused.push(json!({"case":case,"reason":reason}));
        }
        println!(
            "{}",
            json!({"case":case,"passes_analytic_gate":record["passes_analytic_gate"],
                "passes_end_to_end_gate":record["passes_end_to_end_gate"],
                "annotator_exposed":record["annotator_exposed"],"assisted":record["assisted"]})
        );
        if corpus == "photo" {
            crop_records.push(record);
        } else {
            full_records.push(record);
        }
    }

    let report = json!({"schema_version":1,"solver":ASSISTED.name,
        "protocol_id":assisted_replay::PROTOCOL_ID,"workspace_id":workspace_id,
        "dry_run":dry_run,"assisted_result_claimable":!dry_run,
        "domain":"encoded RGB; controlled encoded composites",
        "input_policy":"assisted_human_labels_v1","color_gate":frozen::color_gate_contract(),
        "harness_limit_bytes":HARNESS_LIMIT,"harness_estimate":"256*full_image_pixels+64MiB",
        "photo_summary":frozen::summary_for(&crop_records,ASSISTED),
        "full_image_summary":frozen::summary_for(&full_records,ASSISTED),
        "reference_provenance":crop_references,"reference_manifest":full_references,
        "physical_translucency_reference_available":false,
        "photographic_recompositions":crop_records,"full_images":full_records,
        "refused":refused,"exposed":exposed,
        "solver_input_audit":{"workspace":workspace_dir.to_string_lossy(),
            "reference_directories_metrics_only":[crops.to_string_lossy(),full_dir.to_string_lossy()]}});
    std::fs::write(
        output.join("metrics.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::copy(
        full_dir.join("ATTRIBUTION.md"),
        output.join("ATTRIBUTION.md"),
    )
    .unwrap();
    println!(
        "photographic {}\nfull images {}",
        report["photo_summary"], report["full_image_summary"]
    );
}
