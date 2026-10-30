//! The Phase 9I assisted scoring seam: the one test-side SHA-256, the
//! workspace-only solver inputs, the scoreability decision, the replay hash
//! audit, the assisted input policy and the refusal record.
//!
//! The committed `tests/fixtures/assisted/drive-synthetic-replay.json` is the
//! byte-for-byte replay the 2026-09-16 production drive wrote. Reproducing all
//! fourteen of its per-event label-plane hashes here proves that the scorer
//! rebuilds exactly what the app rendered, without the app.
use doctored_core::assisted_replay::{self, Origin};
use image::{GrayImage, Luma, RgbImage, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[allow(dead_code)]
#[path = "../examples/phase6_matting_quality.rs"]
mod qualification;
use crate::scoring;

use scoring::{sha256_hex, ReplayInput};

/// The drive workspace's own declarations for the fixture's case, copied from
/// `assisted-workspace-drive/workspace/manifest.json`.
const DRIVE_ANON: &str = "788218745725";
const DRIVE_WORKSPACE: &str = "ef0f23bd-120d-4ed9-8963-a400af609ef3";
const DRIVE_TASK_CARD: &str = "da1e0021d7cf26b00255a416e5afca00f1edd4be7b72fb30915dab41ab73394b";
const DRIVE_SOURCE_PNG: &str = "f0e309d70cc02cd17b8ae12357db869f8973a4d09c5b60d9e181294c26f5f4f4";
const DRIVE_SOURCE_PLANE: &str = "852a78c5b40017d11d0da986d1183bf020ad80cea7fe9e271f434706550a48af";
const DRIVE_PRIOR_PNG: &str = "ceed330eb4c865e81ae5256c1d52efb641846c58538946aa2d72b4e2f1c6af1d";
const DRIVE_PRIOR_PLANE: &str = "c3cc84d14e9fa76d0353f9758b711e73a898e4b69766ccf576e8f8abbfccee9a";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/assisted/drive-synthetic-replay.json")
}

fn drive_case() -> scoring::CaseInputs {
    scoring::CaseInputs {
        anonymous_id: DRIVE_ANON.into(),
        width: 2304,
        height: 3456,
        source_png_sha256: DRIVE_SOURCE_PNG.into(),
        source_plane_sha256: DRIVE_SOURCE_PLANE.into(),
        prior_png_sha256: DRIVE_PRIOR_PNG.into(),
        prior_plane_sha256: DRIVE_PRIOR_PLANE.into(),
    }
}

// ------------------------------------------------------------------ sha256 --

#[test]
fn sha256_matches_the_published_vectors() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

// ------------------------------------------------------------- the fixture --

#[test]
fn the_drive_replay_reproduces_every_recorded_plane_hash() {
    let document = std::fs::read_to_string(fixture_path()).expect("the committed fixture");
    let replay = assisted_replay::parse(&document).expect("the app wrote a protocol-valid replay");
    assert_eq!(replay.events.len(), 14);
    assert_eq!(replay.origin, Origin::Synthetic);
    assert_eq!(replay.events[12].result, "rejected:rasterizer_limit");
    // A refused stroke costs a gesture and leaves the plane untouched.
    assert_eq!(
        replay.events[12].label_plane_sha256,
        replay.events[11].label_plane_sha256
    );
    assert_eq!(scoring::verify_replay_hashes(&replay), Ok(14));
}

#[test]
fn a_synthetic_session_is_refused_unless_the_dry_run_allows_it() {
    let document = std::fs::read_to_string(fixture_path()).expect("the committed fixture");
    let replay = assisted_replay::parse(&document).expect("the fixture parses");
    let summary = assisted_replay::summary(&replay);
    let case = drive_case();
    let refusal = scoring::check_scoreable(
        &replay,
        &summary,
        &case,
        DRIVE_WORKSPACE,
        DRIVE_TASK_CARD,
        false,
    )
    .expect_err("a synthetic session is never a measurement");
    assert!(refusal.contains("synthetic"), "{refusal}");
    assert_eq!(
        scoring::check_scoreable(
            &replay,
            &summary,
            &case,
            DRIVE_WORKSPACE,
            DRIVE_TASK_CARD,
            true
        ),
        Ok(())
    );
}

// ---------------------------------------------------- scoreability refusals --

fn digest(tag: char) -> String {
    std::iter::repeat_n(tag, 64).collect()
}

fn human_case() -> scoring::CaseInputs {
    scoring::CaseInputs {
        anonymous_id: "0123456789ab".into(),
        width: 8,
        height: 8,
        source_png_sha256: digest('0'),
        source_plane_sha256: digest('1'),
        prior_png_sha256: digest('2'),
        prior_plane_sha256: digest('3'),
    }
}

/// A protocol-valid human session: one accepted stroke, then `terminal`.
/// `terminal` of `None` leaves the session in progress.
fn human_document(terminal: Option<&str>) -> Value {
    let mut events = vec![
        json!({"index":0,"gesture_index":1,"kind":"stroke","origin":"human",
        "tool":"foreground","diameter":9,"points":[[1,1],[4,4]],"elapsed_ms":100,
        "result":"accepted","detail":Value::Null,"label_plane_sha256":digest('b')}),
    ];
    let exhausted = terminal == Some("budget_exhausted");
    if let Some(kind) = terminal {
        events.push(
            json!({"index":1,"gesture_index":Value::Null,"kind":kind,"origin":"human",
            "tool":Value::Null,"diameter":Value::Null,"points":Value::Null,
            "elapsed_ms":if exhausted {300_000} else {200},"result":"accepted",
            "detail":Value::Null,"label_plane_sha256":digest('c')}),
        );
    }
    let outcome = match terminal {
        Some("submit") => json!("submitted"),
        Some("budget_exhausted") => json!("budget_exhausted"),
        Some("abandon") => json!("incomplete"),
        _ => Value::Null,
    };
    json!({
        "replay_schema_version":1,"protocol_id":"phase9i-assisted-edges-v1",
        "workspace_id":"workspace","session_id":"session","case_id":"0123456789ab",
        "width":8,"height":8,"origin":"human","human_origin":true,"scoreable":true,
        "annotator_id":"tester","annotator_reports_prior_exposure":false,
        "blinding_attestation":{"source_only":true,"text":"source only",
            "attested_at_utc":"2026-09-16T00:00:00Z"},
        "displayed_assets":{"source_png_sha256":digest('0'),"source_plane_sha256":digest('1'),
            "prior_png_sha256":digest('2'),"prior_plane_sha256":digest('3'),
            "task_card_sha256":digest('4')},
        "budget":{"seconds":300,"modifying_gestures":20,"brush_diameters":[3,9,27,81],
            "max_stroke_points":256},
        "started_at_utc":"2026-09-16T00:00:00Z",
        "ended_at_utc":if terminal.is_some() {json!("2026-09-16T00:05:00Z")} else {Value::Null},
        "elapsed_ms":if exhausted {300_000} else {200},
        "modifying_gestures":1,"outcome":outcome,"events":events,
        "label_plane":{"encoding":"trimap_u8_0_128_255","sha256":digest('c')},
        "write_planes":Value::Null,
        "program":{"app_version":"test","rasterizer":"rz_trimap_strokes/v1"}
    })
}

fn human_replay(terminal: Option<&str>) -> assisted_replay::Replay {
    assisted_replay::parse(&human_document(terminal).to_string()).expect("a valid human replay")
}

fn scoreable(replay: &assisted_replay::Replay, case: &scoring::CaseInputs) -> Result<(), String> {
    let summary = assisted_replay::summary(replay);
    scoring::check_scoreable(replay, &summary, case, "workspace", &digest('4'), false)
}

#[test]
fn every_recorded_outcome_is_scoreable() {
    for terminal in ["submit", "budget_exhausted", "abandon"] {
        assert_eq!(
            scoreable(&human_replay(Some(terminal)), &human_case()),
            Ok(()),
            "{terminal}"
        );
    }
}

#[test]
fn an_unscoreable_replay_is_refused_with_its_own_reason() {
    let case = human_case();

    let refusal = scoreable(&human_replay(None), &case).expect_err("in progress");
    assert!(refusal.contains("in progress"), "{refusal}");

    let mut synthetic = human_replay(Some("submit"));
    synthetic.origin = Origin::Synthetic;
    let refusal = scoreable(&synthetic, &case).expect_err("synthetic");
    assert!(refusal.contains("synthetic"), "{refusal}");

    let mut unscoreable = human_replay(Some("submit"));
    unscoreable.scoreable = false;
    let refusal = scoreable(&unscoreable, &case).expect_err("unscoreable");
    assert!(refusal.contains("unscoreable"), "{refusal}");

    let mut unattested = human_replay(Some("submit"));
    unattested.blinding_attestation = None;
    let refusal = scoreable(&unattested, &case).expect_err("no attestation");
    assert!(refusal.contains("blinding attestation"), "{refusal}");

    let mut other_case = human_replay(Some("submit"));
    other_case.case_id = "cafecafecafe".into();
    let refusal = scoreable(&other_case, &case).expect_err("case id");
    assert!(refusal.contains("case_id"), "{refusal}");

    let mut other_workspace = human_replay(Some("submit"));
    other_workspace.workspace_id = "elsewhere".into();
    let refusal = scoreable(&other_workspace, &case).expect_err("workspace id");
    assert!(refusal.contains("another workspace"), "{refusal}");

    let mut resized = human_replay(Some("submit"));
    resized.width = 9;
    let refusal = scoreable(&resized, &case).expect_err("dimensions");
    assert!(refusal.contains("dimensions"), "{refusal}");

    for field in [
        "source_png_sha256",
        "source_plane_sha256",
        "prior_png_sha256",
        "prior_plane_sha256",
    ] {
        let mut replay = human_replay(Some("submit"));
        let assets = &mut replay.displayed_assets;
        match field {
            "source_png_sha256" => assets.source_png_sha256 = digest('f'),
            "source_plane_sha256" => assets.source_plane_sha256 = digest('f'),
            "prior_png_sha256" => assets.prior_png_sha256 = digest('f'),
            _ => assets.prior_plane_sha256 = digest('f'),
        }
        let refusal = scoreable(&replay, &case).expect_err(field);
        assert!(refusal.contains(field), "{refusal}");
    }

    let mut card = human_replay(Some("submit"));
    card.displayed_assets.task_card_sha256 = digest('f');
    let refusal = scoreable(&card, &case).expect_err("task card");
    assert!(refusal.contains("task card"), "{refusal}");

    let mut planeless = human_replay(Some("submit"));
    planeless.label_plane = None;
    let refusal = scoreable(&planeless, &case).expect_err("no label plane");
    assert!(refusal.contains("no label plane"), "{refusal}");
}

// ----------------------------------------------------------- solver inputs --

struct Workspace {
    _root: TempDir,
    path: PathBuf,
    case: scoring::CaseInputs,
}

/// A one-case workspace with a 4x4 RGB8 source and its Gray8 prior, hashed
/// with the same implementation the scorer re-checks them with.
fn workspace(anonymous: &str) -> Workspace {
    let root = TempDir::new().expect("tempdir");
    let path = root.path().to_path_buf();
    let directory = path.join("cases").join(anonymous);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::create_dir_all(path.join("replays")).unwrap();
    let source = RgbImage::from_fn(4, 4, |x, y| {
        image::Rgb([(x * 17) as u8, (y * 23) as u8, (x * y) as u8])
    });
    let prior = GrayImage::from_fn(4, 4, |x, y| Luma([(x * 60 + y) as u8]));
    source.save(directory.join("source.png")).unwrap();
    prior.save(directory.join("prior.png")).unwrap();
    let case = scoring::CaseInputs {
        anonymous_id: anonymous.into(),
        width: 4,
        height: 4,
        source_png_sha256: sha256_hex(&std::fs::read(directory.join("source.png")).unwrap()),
        source_plane_sha256: sha256_hex(source.as_raw()),
        prior_png_sha256: sha256_hex(&std::fs::read(directory.join("prior.png")).unwrap()),
        prior_plane_sha256: sha256_hex(prior.as_raw()),
    };
    Workspace {
        _root: root,
        path,
        case,
    }
}

#[test]
fn solver_inputs_open_exactly_the_three_case_files() {
    let fixture = workspace("0123456789ab");
    let inputs = scoring::solver_inputs(&fixture.path, &fixture.case).expect("clean workspace");
    assert_eq!(inputs.source.dimensions(), (4, 4));
    assert_eq!(inputs.prior.dimensions(), (4, 4));
    assert!(matches!(inputs.replay, ReplayInput::Missing));
    assert_eq!(
        inputs.read_files,
        vec![
            PathBuf::from("cases/0123456789ab/source.png"),
            PathBuf::from("cases/0123456789ab/prior.png"),
        ]
    );

    std::fs::write(
        fixture.path.join("replays/0123456789ab.json"),
        human_document(Some("submit")).to_string(),
    )
    .unwrap();
    let inputs = scoring::solver_inputs(&fixture.path, &fixture.case).expect("clean workspace");
    assert_eq!(
        inputs.read_files,
        vec![
            PathBuf::from("cases/0123456789ab/source.png"),
            PathBuf::from("cases/0123456789ab/prior.png"),
            PathBuf::from("replays/0123456789ab.json"),
        ]
    );
    match inputs.replay {
        ReplayInput::Parsed { replay, .. } => assert_eq!(replay.case_id, "0123456789ab"),
        _ => panic!("the replay parses"),
    }

    std::fs::write(
        fixture.path.join("replays/0123456789ab.json"),
        "{not json at all",
    )
    .unwrap();
    let inputs = scoring::solver_inputs(&fixture.path, &fixture.case).expect("clean workspace");
    assert!(matches!(inputs.replay, ReplayInput::Malformed { .. }));
}

#[test]
fn solver_inputs_refuse_an_unexpected_storage_extent_or_hash() {
    let fixture = workspace("0123456789ab");
    let directory = fixture.path.join("cases/0123456789ab");

    RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255]))
        .save(directory.join("source.png"))
        .unwrap();
    let error = scoring::solver_inputs(&fixture.path, &fixture.case)
        .err()
        .expect("an RGBA source refuses");
    assert_eq!(error, "source is not RGB8 with no alpha");

    let clean = workspace("0123456789ab");
    GrayImage::new(5, 5)
        .save(clean.path.join("cases/0123456789ab/prior.png"))
        .unwrap();
    let error = scoring::solver_inputs(&clean.path, &clean.case)
        .err()
        .expect("a wrong prior extent refuses");
    assert!(error.contains("prior dimensions"), "{error}");

    let mut tampered = workspace("0123456789ab");
    tampered.case.source_png_sha256 = digest('f');
    let error = scoring::solver_inputs(&tampered.path, &tampered.case)
        .err()
        .expect("a wrong hash refuses");
    assert!(error.contains("source.png sha256"), "{error}");
}

// ------------------------------------------------------- the assisted policy --

fn output(alpha: GrayImage) -> Result<qualification::AlphaResult, String> {
    Ok(qualification::AlphaResult {
        foreground: None,
        alpha,
        confidence: None,
        diagnostics: json!({}),
    })
}

fn evaluate(estimate: qualification::EstimateAlpha, labels: &GrayImage) -> Value {
    let truth = GrayImage::from_raw(9, 1, vec![0, 0, 0, 0, 128, 255, 255, 255, 255]).unwrap();
    let source = RgbaImage::from_fn(9, 1, |x, _| {
        let a = truth.get_pixel(x, 0)[0];
        Rgba([a, a, a, 255])
    });
    qualification::evaluate_candidate_with_labels(
        (Path::new("unused"), "contract"),
        &source,
        &truth,
        &[[1.0; 3]; 9],
        "binary",
        false,
        qualification::Candidate {
            protect_foreground: false,
            input_policy: qualification::InputPolicy::AssistedHumanLabelsV1,
            name: "assisted-test",
            estimate,
        },
        Some(labels),
    )
}

fn human_labels() -> GrayImage {
    GrayImage::from_raw(9, 1, vec![0, 128, 128, 128, 128, 128, 128, 128, 255]).unwrap()
}

#[test]
fn human_labels_reach_the_candidate_as_hard_constraints() {
    let report = evaluate(
        |_, colors, w, h, user, _| {
            assert_eq!(user[0], 0);
            assert_eq!(user[4], 128);
            assert_eq!(user[8], 255);
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
            )
        },
        &human_labels(),
    );
    assert!(report["alpha_candidate"].get("refusal").is_none());
    assert_eq!(report["input_policy"]["version"], 1);
    assert_eq!(
        report["input_policy"]["user_constraints"],
        "assisted_human_labels_v1"
    );
    assert_eq!(
        report["input_policy"]["human_labels_used_as_hard_constraints"],
        true
    );
    assert_eq!(
        report["input_policy"]["automatic_labels_used_as_hard_constraints"],
        false
    );
    assert_eq!(report["input_policy"]["labeled_foreground_pixels"], 1);
    assert_eq!(report["input_policy"]["labeled_background_pixels"], 1);
    assert_eq!(report["input_policy"]["unknown_pixels"], 7);
    assert!(report.get("legacy_automatic_constraint_audit").is_some());
}

#[test]
fn a_candidate_that_overwrites_a_human_label_refuses() {
    let report = evaluate(
        |_, _, w, h, _, _| output(GrayImage::new(w, h)),
        &human_labels(),
    );
    assert!(report["alpha_candidate"]["refusal"]
        .as_str()
        .unwrap()
        .contains("known categorical"));
    assert_eq!(report["passes_analytic_gate"], false);
    assert_eq!(report["passes_end_to_end_gate"], false);
}

// ------------------------------------------------------- the refusal record --

#[test]
fn a_refusal_record_reads_as_a_failed_refused_case() {
    let truth = GrayImage::from_raw(9, 1, vec![0, 0, 0, 0, 128, 255, 255, 255, 255]).unwrap();
    let prior = GrayImage::from_raw(9, 1, vec![0, 0, 0, 0, 0, 255, 255, 255, 255]).unwrap();
    let before = qualification::quality::alpha_metrics(&truth, &prior);
    let record = scoring::refusal_record(
        "full-x-checker-binary",
        (9, 1),
        before.clone(),
        true,
        "assisted_tiled_soft_prior_v1",
        "no replay",
    );
    assert_eq!(record["case"], "full-x-checker-binary");
    assert_eq!(record["passes_end_to_end_gate"], false);
    assert_eq!(record["passes_analytic_gate"], false);
    assert_eq!(record["dimensions"], json!([9, 1]));
    assert_eq!(record["before"], record["after"]);
    assert_eq!(record["before"], before);
    assert_eq!(record["alpha_candidate"]["refusal"], "no replay");
    assert_eq!(record["oracle_alpha_color"]["passes_color_gate"], false);
    assert_eq!(record["estimated_alpha_color"]["passes_color_gate"], false);
    assert_eq!(
        record["estimated_alpha_color"]["alpha_candidate_refused"],
        true
    );
    assert_eq!(record["solver"], "assisted_tiled_soft_prior_v1");
}
