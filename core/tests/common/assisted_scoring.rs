//! Shared inputs and refusals for the Phase 9I assisted scoring path.
//!
//! This module is the seam between an exported assisted workspace and the
//! frozen evaluator. It opens ONLY the three workspace files a case owns —
//! `cases/<anon>/source.png`, `cases/<anon>/prior.png` and
//! `replays/<anon>.json` — re-checks every hash the workspace declared, decides
//! whether a replay may be scored at all, audits the app-recorded per-event
//! label-plane hashes against `assisted_replay::label_plane`, and builds the
//! refusal record shape `scripts/phase9i_gate_v2_rescore.py` already reads.
//! Nothing here scores, and nothing here reads a reference plane.
#![allow(dead_code)]

use doctored_core::assisted_replay::{self, Event, EventKind, Origin, Outcome, Replay, Summary};
use image::{DynamicImage, GrayImage, RgbImage};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[path = "sha256.rs"]
mod sha256;
pub use sha256::sha256_hex;

/// One workspace manifest case entry, as the exporter wrote it.
pub struct CaseInputs {
    pub anonymous_id: String,
    pub width: u32,
    pub height: u32,
    pub source_png_sha256: String,
    pub source_plane_sha256: String,
    pub prior_png_sha256: String,
    pub prior_plane_sha256: String,
}

fn text(entry: &Value, key: &str) -> Result<String, String> {
    entry[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("case entry is missing a string {key}"))
}

fn extent(entry: &Value, key: &str) -> Result<u32, String> {
    let value = entry[key]
        .as_u64()
        .ok_or_else(|| format!("case entry is missing an integer {key}"))?;
    u32::try_from(value).map_err(|_| format!("case entry {key} is out of range"))
}

/// Read one manifest case entry. Only the fields the scorer re-checks.
pub fn case_inputs(entry: &Value) -> Result<CaseInputs, String> {
    Ok(CaseInputs {
        anonymous_id: text(entry, "anonymous_id")?,
        width: extent(entry, "width")?,
        height: extent(entry, "height")?,
        source_png_sha256: text(entry, "source_png_sha256")?,
        source_plane_sha256: text(entry, "source_plane_sha256")?,
        prior_png_sha256: text(entry, "prior_png_sha256")?,
        prior_plane_sha256: text(entry, "prior_plane_sha256")?,
    })
}

/// What `replays/<anon>.json` turned out to be. A missing or malformed replay
/// is a scored refusal, never a skipped case.
// One of these exists per case, built and consumed in the same loop iteration,
// so boxing the parsed document would only add an allocation and a deref.
#[allow(clippy::large_enum_variant)]
pub enum ReplayInput {
    Missing,
    Malformed {
        sha256: String,
        error: String,
    },
    Parsed {
        sha256: String,
        replay: Replay,
        summary: Summary,
    },
}

/// The complete solver-visible input set for one case, plus the audit of which
/// workspace files were opened to build it.
pub struct SolverInputs {
    pub source: RgbImage,
    pub prior: GrayImage,
    pub replay: ReplayInput,
    /// Workspace-relative, exactly the files opened.
    pub read_files: Vec<PathBuf>,
}

fn check_digest(data: &[u8], declared: &str, what: &str) -> Result<(), String> {
    let measured = sha256_hex(data);
    if measured == declared {
        return Ok(());
    }
    Err(format!(
        "{what} sha256 {measured} does not match the declared {declared}"
    ))
}

/// Open the three workspace files one case owns and verify every declared
/// hash and extent. No reference file and no other workspace file is read.
pub fn solver_inputs(workspace: &Path, case: &CaseInputs) -> Result<SolverInputs, String> {
    let mut read_files = Vec::new();
    let source_relative = Path::new("cases")
        .join(&case.anonymous_id)
        .join("source.png");
    let prior_relative = Path::new("cases")
        .join(&case.anonymous_id)
        .join("prior.png");
    let replay_relative = Path::new("replays").join(format!("{}.json", case.anonymous_id));

    let source_bytes =
        std::fs::read(workspace.join(&source_relative)).map_err(|e| format!("source.png: {e}"))?;
    read_files.push(source_relative);
    let source = match image::load_from_memory(&source_bytes)
        .map_err(|e| format!("source.png does not decode: {e}"))?
    {
        DynamicImage::ImageRgb8(image) => image,
        _ => return Err("source is not RGB8 with no alpha".into()),
    };
    if source.dimensions() != (case.width, case.height) {
        return Err("source dimensions do not match the case entry".into());
    }
    check_digest(&source_bytes, &case.source_png_sha256, "source.png")?;
    check_digest(
        source.as_raw(),
        &case.source_plane_sha256,
        "the source plane",
    )?;

    let prior_bytes =
        std::fs::read(workspace.join(&prior_relative)).map_err(|e| format!("prior.png: {e}"))?;
    read_files.push(prior_relative);
    let prior = match image::load_from_memory(&prior_bytes)
        .map_err(|e| format!("prior.png does not decode: {e}"))?
    {
        DynamicImage::ImageLuma8(image) => image,
        _ => return Err("prior is not Gray8 with no alpha".into()),
    };
    if prior.dimensions() != (case.width, case.height) {
        return Err("prior dimensions do not match the case entry".into());
    }
    check_digest(&prior_bytes, &case.prior_png_sha256, "prior.png")?;
    check_digest(prior.as_raw(), &case.prior_plane_sha256, "the prior plane")?;

    let replay_path = workspace.join(&replay_relative);
    let replay = if replay_path.exists() {
        let bytes = std::fs::read(&replay_path).map_err(|e| format!("replay: {e}"))?;
        read_files.push(replay_relative);
        let digest = sha256_hex(&bytes);
        let document =
            String::from_utf8(bytes).map_err(|_| "the replay document is not UTF-8".to_string())?;
        match assisted_replay::parse(&document) {
            Ok(replay) => {
                let summary = assisted_replay::summary(&replay);
                ReplayInput::Parsed {
                    sha256: digest,
                    replay,
                    summary,
                }
            }
            Err(error) => ReplayInput::Malformed {
                sha256: digest,
                error,
            },
        }
    } else {
        ReplayInput::Missing
    };

    Ok(SolverInputs {
        source,
        prior,
        replay,
        read_files,
    })
}

/// Decision 3: may this replay be scored at all? Every refusal names its own
/// reason; every recorded outcome — submitted, budget-exhausted or incomplete
/// — is accepted, because an unfinished plane is still what the annotator drew.
pub fn check_scoreable(
    replay: &Replay,
    summary: &Summary,
    case: &CaseInputs,
    workspace_id: &str,
    task_card_sha256: &str,
    allow_synthetic: bool,
) -> Result<(), String> {
    if !summary.is_final {
        return Err("the session is still in progress".into());
    }
    if replay.origin != Origin::Human {
        if !allow_synthetic {
            return Err("a synthetic session is never a measurement".into());
        }
    } else {
        if !replay.scoreable || !summary.scoreable {
            return Err("the replay declares itself unscoreable".into());
        }
        if replay.blinding_attestation.is_none() {
            return Err("a human session needs a blinding attestation".into());
        }
    }
    if replay.case_id != case.anonymous_id {
        return Err("the replay case_id is not this case".into());
    }
    if replay.workspace_id != workspace_id {
        return Err("the replay belongs to another workspace".into());
    }
    if (replay.width, replay.height) != (case.width, case.height) {
        return Err("the replay dimensions are not the case dimensions".into());
    }
    let assets = &replay.displayed_assets;
    for (shown, declared, what) in [
        (
            &assets.source_png_sha256,
            &case.source_png_sha256,
            "source_png_sha256",
        ),
        (
            &assets.source_plane_sha256,
            &case.source_plane_sha256,
            "source_plane_sha256",
        ),
        (
            &assets.prior_png_sha256,
            &case.prior_png_sha256,
            "prior_png_sha256",
        ),
        (
            &assets.prior_plane_sha256,
            &case.prior_plane_sha256,
            "prior_plane_sha256",
        ),
    ] {
        if shown != declared {
            return Err(format!("the displayed {what} is not the case's"));
        }
    }
    if assets.task_card_sha256 != task_card_sha256 {
        return Err("the displayed task card is not the workspace's".into());
    }
    if replay.label_plane.is_none() {
        return Err("the replay records no label plane".into());
    }
    Ok(())
}

fn is_modifying(kind: EventKind) -> bool {
    matches!(kind, EventKind::Stroke | EventKind::Undo | EventKind::Redo)
}

fn terminal_outcome(kind: EventKind) -> Option<Outcome> {
    match kind {
        EventKind::Submit => Some(Outcome::Submitted),
        EventKind::BudgetExhausted => Some(Outcome::BudgetExhausted),
        EventKind::Abandon => Some(Outcome::Incomplete),
        _ => None,
    }
}

/// The same session truncated after event `upto`, exactly as the app held it
/// when it recorded that event's label-plane hash.
pub fn prefix_replay(replay: &Replay, upto: usize) -> Replay {
    let mut prefix = replay.clone();
    prefix.events.truncate(upto + 1);
    let events: &[Event] = &prefix.events;
    prefix.modifying_gestures = events.iter().filter(|e| is_modifying(e.kind)).count() as u32;
    let last = events.last();
    prefix.outcome = last.and_then(|event| terminal_outcome(event.kind));
    prefix.elapsed_ms = last.map_or(0, |event| event.elapsed_ms);
    if prefix.outcome.is_none() {
        prefix.ended_at_utc = None;
    }
    prefix
}

/// Reproduce every per-event label-plane hash the app recorded, plus the
/// document's own `label_plane.sha256`, through the one rasterizer. Returns the
/// number of events verified.
pub fn verify_replay_hashes(replay: &Replay) -> Result<usize, String> {
    let (width, height) = (replay.width, replay.height);
    let declared = replay
        .label_plane
        .as_ref()
        .ok_or("the replay records no label plane")?;
    for index in 0..replay.events.len() {
        let plane =
            assisted_replay::label_plane(&prefix_replay(replay, index), width, height, None)
                .map_err(|e| format!("event {index} does not rasterize: {e}"))?;
        let measured = sha256_hex(&plane);
        if measured != replay.events[index].label_plane_sha256 {
            return Err(format!(
                "event {index} label plane {measured} does not match the recorded {}",
                replay.events[index].label_plane_sha256
            ));
        }
    }
    let last = replay
        .events
        .last()
        .ok_or("a scoreable replay records at least one event")?;
    if last.label_plane_sha256 != declared.sha256 {
        return Err("the declared label plane is not the last event's".into());
    }
    let whole = assisted_replay::label_plane(replay, width, height, None)
        .map_err(|e| format!("the replay does not rasterize: {e}"))?;
    let measured = sha256_hex(&whole);
    if measured != declared.sha256 {
        return Err(format!(
            "the replay label plane {measured} does not match the recorded {}",
            declared.sha256
        ));
    }
    Ok(replay.events.len())
}

/// The record shape `score_run` already reads as a failed, refused case: the
/// prior stands in for `after` so the diagnostic metrics stay indexable, and
/// every gate field is false.
pub fn refusal_record(
    case: &str,
    dimensions: (u32, u32),
    before: Value,
    has_fractional_reference: bool,
    solver: &str,
    reason: &str,
) -> Value {
    json!({
        "case": case,
        "passes_end_to_end_gate": false,
        "passes_analytic_gate": false,
        "has_fractional_reference": has_fractional_reference,
        "dimensions": [dimensions.0, dimensions.1],
        "before": before.clone(),
        "after": before,
        "alpha_candidate": {"refusal": reason,
            "reported_alpha": "unchanged prior for diagnostic metrics only"},
        "oracle_alpha_color": {"passes_color_gate": false, "refusal": reason},
        "estimated_alpha_color": {"passes_color_gate": false, "refusal": reason,
            "alpha_candidate_refused": true},
        "solver": solver,
    })
}
