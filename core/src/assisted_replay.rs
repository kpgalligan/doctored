//! Strict schema, bookkeeping and label-plane replay for the Phase 9I
//! assisted edge annotation protocol (`phase9i-assisted-edges-v1`). One replay
//! document describes one case's whole session; the app is its only writer and
//! this module is its only reader. Nothing here scores, hashes or renders a
//! matting candidate: a replay is validated, its active stroke set is
//! reconstructed, and that set is rasterized through the existing
//! `trimap_strokes` constructor.
//!
//! The replay document (this table IS the schema; `deny_unknown_fields`):
//!
//! | Field | Type | Rule |
//! |---|---|---|
//! | `replay_schema_version` | u32 | exactly 1 |
//! | `protocol_id` | string | exactly [`PROTOCOL_ID`] |
//! | `workspace_id` / `session_id` / `case_id` | string | non-empty |
//! | `width` / `height` | u32 | nonzero, within the canvas pixel limit |
//! | `origin` | `"human"` \| `"synthetic"` | every event repeats it |
//! | `human_origin` | bool | equals `origin == "human"` |
//! | `scoreable` | bool | equals `human_origin` |
//! | `annotator_id` | string \| null | non-empty for human, null for synthetic |
//! | `annotator_reports_prior_exposure` | bool \| null | declared exposure |
//! | `blinding_attestation` | object \| null | human requires `source_only: true` |
//! | `displayed_assets` | object | five 64-hex sha256 of what was shown |
//! | `budget` | object | must equal [`BUDGET_MS`]/[`BUDGET_GESTURES`]/[`DIAMETERS`]/[`MAX_STROKE_POINTS`] |
//! | `started_at_utc` | string | non-empty |
//! | `ended_at_utc` | string \| null | present once the session finished |
//! | `elapsed_ms` | u64 | session clock at the last write |
//! | `modifying_gestures` | u32 | equals the counted modifying events |
//! | `outcome` | `"submitted"` \| `"budget_exhausted"` \| `"incomplete"` \| null | null iff no terminal event |
//! | `events` | array | see below |
//! | `label_plane` | object \| null | `encoding` is `trimap_u8_0_128_255` |
//! | `write_planes` | object \| null | labels-only collection: constant planes |
//! | `program` | object | app version and `rasterizer` identity |
//!
//! One event:
//!
//! | Field | Type | Rule |
//! |---|---|---|
//! | `index` | u32 | sequential from 0 |
//! | `gesture_index` | u32 \| null | 1-based on stroke/undo/redo, null elsewhere, at most [`BUDGET_GESTURES`] |
//! | `kind` | stroke \| undo \| redo \| view \| submit \| budget_exhausted \| abandon | at most one terminal, last only |
//! | `origin` | `"human"` \| `"synthetic"` | equals the replay's |
//! | `tool` | foreground \| background \| unknown \| null | required on stroke, null elsewhere |
//! | `diameter` | u32 \| null | required on stroke, one of [`DIAMETERS`] |
//! | `points` | `[[x,y],…]` \| null | required on stroke, 1…[`MAX_STROKE_POINTS`] integer canvas points |
//! | `elapsed_ms` | u64 | non-decreasing; `<= BUDGET_MS` on an accepted modifying event |
//! | `result` | `accepted` \| `rejected:<reason>` | reason in [`REJECT_REASONS`], strokes only |
//! | `detail` | string \| null | free text (a view event's description) |
//! | `label_plane_sha256` | string | 64-hex; the host hashes, the core never does |
//!
//! Undo needs a non-empty active stack and redo a non-empty redo stack; an
//! accepted stroke clears the redo stack. A rejected stroke still costs a
//! gesture and is not applied, and `rejected:time_budget` additionally
//! requires `elapsed_ms > BUDGET_MS`. Every sha field is checked for SYNTAX
//! only — the core never reads a PNG and never computes a digest.
//!
//! The label plane is an all-128 base with the active strokes applied in
//! order, as the version-1 `trimap_strokes` payload with `radius = diameter/2`
//! and points at pixel centers `[x+0.5, y+0.5]`. That constructor owns the
//! capsule geometry and its 64-million-visit cap; a refusal from it is
//! reported with the `rasterizer_limit: ` prefix so the host can record the
//! gesture as `rejected:rasterizer_limit` instead of treating it as a bug.
//!
//! Strict `deny_unknown_fields` parse of the whole protocol (budget equality, sequential indices, a
//! non-decreasing clock, 1-based gesture numbering, stroke geometry inside the canvas, undo/redo
//! stack legality, a single trailing terminal event and the matching outcome, 64-hex hash syntax),
//! exact gesture/outcome bookkeeping, and the reconstructed active stroke set rasterized through
//! `trimap_strokes::apply_strokes` — no capsule math is duplicated, and an `apply_strokes` refusal
//! is reported with a `rasterizer_limit: ` prefix so a host can record a counted rejection instead
//! of a bug. The core never hashes, never decodes a PNG and never scores. Scoring is
//! `core/examples/phase9i_assisted_quality.rs` over `core/tests/common/assisted_scoring.rs`, and
//! `core/tests/common/sha256.rs` is the ONE test-side SHA-256 every `tests`/`examples` caller uses.

use crate::{
    job_control::{DrJobControl, JobContext},
    trimap_strokes,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Protocol identity every replay of this experiment declares.
pub const PROTOCOL_ID: &str = "phase9i-assisted-edges-v1";
/// Five minutes of session clock, per case.
pub const BUDGET_MS: u64 = 300_000;
/// Twenty modifying gestures (stroke, undo, redo), per case.
pub const BUDGET_GESTURES: u32 = 20;
/// The four offered brush diameters, in native pixels.
pub const DIAMETERS: [u32; 4] = [3, 9, 27, 81];
/// Recorded samples per stroke; the host decimates a longer drag to this.
pub const MAX_STROKE_POINTS: usize = 256;
/// Replay document byte limit, matching the stroke payload limit.
pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
/// The only two reasons a stroke may be recorded as rejected.
pub const REJECT_REASONS: [&str; 2] = ["time_budget", "rasterizer_limit"];

/// Who produced the session: an actual annotator, or a synthetic driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Human,
    Synthetic,
}

/// How a session ended. `None` on an in-progress document.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Submitted,
    BudgetExhausted,
    Incomplete,
}

/// The three categorical labels a brush can deposit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Foreground,
    Background,
    Unknown,
}
impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
            Self::Unknown => "unknown",
        }
    }
}

/// What an event did to the session.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Stroke,
    Undo,
    Redo,
    View,
    Submit,
    BudgetExhausted,
    Abandon,
}
impl EventKind {
    fn is_modifying(self) -> bool {
        matches!(self, Self::Stroke | Self::Undo | Self::Redo)
    }
    fn terminal_outcome(self) -> Option<Outcome> {
        match self {
            Self::Submit => Some(Outcome::Submitted),
            Self::BudgetExhausted => Some(Outcome::BudgetExhausted),
            Self::Abandon => Some(Outcome::Incomplete),
            _ => None,
        }
    }
}

/// The annotator's source-only declaration; required for a human session.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    pub source_only: bool,
    pub text: String,
    pub attested_at_utc: String,
}

/// Digests of exactly what the window displayed, as the host computed them.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayedAssets {
    pub source_png_sha256: String,
    pub source_plane_sha256: String,
    pub prior_png_sha256: String,
    pub prior_plane_sha256: String,
    pub task_card_sha256: String,
}

/// The declared budget; it must equal this module's constants exactly.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub seconds: u64,
    pub modifying_gestures: u32,
    pub brush_diameters: Vec<u32>,
    pub max_stroke_points: usize,
}

/// The final categorical plane, recorded with the hash the host computed.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelPlaneRecord {
    pub encoding: String,
    pub sha256: String,
}

/// Labels-only collection declares its constant write/protection planes.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WritePlanes {
    pub alpha_write: String,
    pub rgb_write: String,
    pub protection: String,
    pub alpha_write_sha256: String,
    pub rgb_write_sha256: String,
    pub protection_sha256: String,
}

/// The program that produced the replay.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub app_version: String,
    pub rasterizer: String,
}

/// One recorded interaction.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub index: u32,
    pub gesture_index: Option<u32>,
    pub kind: EventKind,
    pub origin: Origin,
    pub tool: Option<Tool>,
    pub diameter: Option<u32>,
    pub points: Option<Vec<[i64; 2]>>,
    pub elapsed_ms: u64,
    pub result: String,
    pub detail: Option<String>,
    pub label_plane_sha256: String,
}

/// One case's whole session.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub replay_schema_version: u32,
    pub protocol_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub case_id: String,
    pub width: u32,
    pub height: u32,
    pub origin: Origin,
    pub human_origin: bool,
    pub scoreable: bool,
    pub annotator_id: Option<String>,
    pub annotator_reports_prior_exposure: Option<bool>,
    pub blinding_attestation: Option<Attestation>,
    pub displayed_assets: DisplayedAssets,
    pub budget: Budget,
    pub started_at_utc: String,
    pub ended_at_utc: Option<String>,
    pub elapsed_ms: u64,
    pub modifying_gestures: u32,
    pub outcome: Option<Outcome>,
    pub events: Vec<Event>,
    pub label_plane: Option<LabelPlaneRecord>,
    pub write_planes: Option<WritePlanes>,
    pub program: Program,
}

/// What a caller can learn about a replay without rasterizing it.
#[derive(Clone, Serialize)]
pub struct Summary {
    #[serde(rename = "final")]
    pub is_final: bool,
    pub outcome: Option<Outcome>,
    pub modifying_gestures: u32,
    pub active_strokes: usize,
    pub human_origin: bool,
    pub scoreable: bool,
    pub elapsed_ms: u64,
}

fn hex64(value: &str, what: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Ok(());
    }
    Err(format!("{what} must be 64 lowercase hex characters"))
}

fn nonempty(value: &str, what: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    Ok(())
}

/// Reconstructed stack state: the accepted strokes still applied, in order,
/// and the accepted strokes an Undo removed that a Redo could restore.
struct Stacks {
    active: Vec<usize>,
    redo: Vec<usize>,
}

fn replay_stacks(r: &Replay) -> Result<Stacks, String> {
    let mut stacks = Stacks {
        active: Vec::new(),
        redo: Vec::new(),
    };
    for (position, event) in r.events.iter().enumerate() {
        match event.kind {
            EventKind::Stroke if event.result == "accepted" => {
                stacks.active.push(position);
                stacks.redo.clear();
            }
            EventKind::Undo => {
                let applied = stacks
                    .active
                    .pop()
                    .ok_or("an undo event needs a non-empty active stroke stack")?;
                stacks.redo.push(applied);
            }
            EventKind::Redo => {
                let restored = stacks
                    .redo
                    .pop()
                    .ok_or("a redo event needs a non-empty redo stack")?;
                stacks.active.push(restored);
            }
            _ => {}
        }
    }
    Ok(stacks)
}

fn check_result(event: &Event) -> Result<(), String> {
    if event.result == "accepted" {
        if event.kind.is_modifying() && event.elapsed_ms > BUDGET_MS {
            return Err("an accepted modifying event must be inside the time budget".into());
        }
        return Ok(());
    }
    let reason = event
        .result
        .strip_prefix("rejected:")
        .filter(|_| event.kind == EventKind::Stroke)
        .ok_or("only a stroke may be rejected, and only as rejected:<reason>")?;
    if !REJECT_REASONS.contains(&reason) {
        return Err(format!("unknown rejection reason {reason}"));
    }
    if reason == "time_budget" && event.elapsed_ms <= BUDGET_MS {
        return Err("rejected:time_budget requires an over-budget elapsed_ms".into());
    }
    Ok(())
}

fn check_stroke_shape(event: &Event, width: u32, height: u32) -> Result<(), String> {
    if event.kind != EventKind::Stroke {
        if event.tool.is_some() || event.diameter.is_some() || event.points.is_some() {
            return Err("only a stroke event carries a tool, diameter and points".into());
        }
        return Ok(());
    }
    let diameter = event.diameter.ok_or("a stroke event needs a diameter")?;
    if event.tool.is_none() {
        return Err("a stroke event needs a tool".into());
    }
    if !DIAMETERS.contains(&diameter) {
        return Err(format!(
            "stroke diameter {diameter} is not one of {DIAMETERS:?}"
        ));
    }
    let points = event.points.as_ref().ok_or("a stroke event needs points")?;
    if points.is_empty() || points.len() > MAX_STROKE_POINTS {
        return Err(format!(
            "a stroke carries 1 to {MAX_STROKE_POINTS} recorded points"
        ));
    }
    for point in points {
        if point[0] < 0
            || point[1] < 0
            || point[0] >= i64::from(width)
            || point[1] >= i64::from(height)
        {
            return Err("every stroke point must be an integer pixel inside the canvas".into());
        }
    }
    Ok(())
}

fn check_events(r: &Replay) -> Result<(), String> {
    let mut gestures = 0u32;
    let mut previous_ms = 0u64;
    let mut terminal = None;
    for (position, event) in r.events.iter().enumerate() {
        if u64::from(event.index) != position as u64 {
            return Err("event indices must be sequential from 0".into());
        }
        if event.origin != r.origin {
            return Err("every event must carry the replay's own origin".into());
        }
        if event.elapsed_ms < previous_ms {
            return Err("event elapsed_ms must not decrease".into());
        }
        previous_ms = event.elapsed_ms;
        hex64(&event.label_plane_sha256, "an event label_plane_sha256")?;
        if event.kind.is_modifying() {
            gestures += 1;
            if event.gesture_index != Some(gestures) {
                return Err("modifying events are numbered 1-based in gesture_index".into());
            }
            if gestures > BUDGET_GESTURES {
                return Err(format!(
                    "a session records at most {BUDGET_GESTURES} modifying gestures"
                ));
            }
        } else if event.gesture_index.is_some() {
            return Err("only a stroke, undo or redo carries a gesture_index".into());
        }
        check_stroke_shape(event, r.width, r.height)?;
        check_result(event)?;
        if terminal.is_some() {
            return Err("a terminal event must be the last event".into());
        }
        terminal = event.kind.terminal_outcome();
    }
    if r.modifying_gestures != gestures {
        return Err("modifying_gestures does not equal the counted gestures".into());
    }
    if r.outcome != terminal {
        return Err("outcome must match the terminal event, and be null without one".into());
    }
    if terminal == Some(Outcome::BudgetExhausted)
        && gestures != BUDGET_GESTURES
        && r.elapsed_ms < BUDGET_MS
    {
        return Err("budget_exhausted requires a spent gesture or time budget".into());
    }
    replay_stacks(r).map(|_| ())
}

fn check_identity(r: &Replay) -> Result<(), String> {
    if r.replay_schema_version != 1 || r.protocol_id != PROTOCOL_ID {
        return Err("unsupported assisted replay schema or protocol".into());
    }
    nonempty(&r.workspace_id, "workspace_id")?;
    nonempty(&r.session_id, "session_id")?;
    nonempty(&r.case_id, "case_id")?;
    nonempty(&r.started_at_utc, "started_at_utc")?;
    trimap_strokes::pixel_count(r.width, r.height)?;
    if r.budget.seconds != BUDGET_MS / 1000
        || r.budget.modifying_gestures != BUDGET_GESTURES
        || r.budget.brush_diameters != DIAMETERS
        || r.budget.max_stroke_points != MAX_STROKE_POINTS
    {
        return Err("the declared budget does not match this protocol".into());
    }
    if r.human_origin != (r.origin == Origin::Human) || r.scoreable != r.human_origin {
        return Err("human_origin and scoreable must follow origin".into());
    }
    match r.origin {
        Origin::Human => {
            let id = r
                .annotator_id
                .as_deref()
                .ok_or("a human session needs an annotator_id")?;
            nonempty(id, "annotator_id")?;
            let attestation = r
                .blinding_attestation
                .as_ref()
                .ok_or("a human session needs a blinding attestation")?;
            if !attestation.source_only {
                return Err("a human session attests source_only".into());
            }
        }
        Origin::Synthetic => {
            if r.annotator_id.is_some() {
                return Err("a synthetic session records no annotator_id".into());
            }
        }
    }
    let assets = &r.displayed_assets;
    for (value, what) in [
        (&assets.source_png_sha256, "source_png_sha256"),
        (&assets.source_plane_sha256, "source_plane_sha256"),
        (&assets.prior_png_sha256, "prior_png_sha256"),
        (&assets.prior_plane_sha256, "prior_plane_sha256"),
        (&assets.task_card_sha256, "task_card_sha256"),
    ] {
        hex64(value, what)?;
    }
    if let Some(plane) = &r.label_plane {
        if plane.encoding != "trimap_u8_0_128_255" {
            return Err("the label plane encoding is trimap_u8_0_128_255".into());
        }
        hex64(&plane.sha256, "the label plane sha256")?;
    }
    if let Some(planes) = &r.write_planes {
        hex64(&planes.alpha_write_sha256, "alpha_write_sha256")?;
        hex64(&planes.rgb_write_sha256, "rgb_write_sha256")?;
        hex64(&planes.protection_sha256, "protection_sha256")?;
    }
    Ok(())
}

/// Parse and fully validate one replay document, in progress or final.
/// Every rule of the module table is enforced here; nothing else in this
/// module re-checks a document it was handed.
pub fn parse(json: &str) -> Result<Replay, String> {
    if json.len() > MAX_JSON_BYTES {
        return Err("assisted replay exceeds 4 MiB".into());
    }
    let replay: Replay =
        serde_json::from_str(json).map_err(|e| format!("invalid assisted replay: {e}"))?;
    check_identity(&replay)?;
    check_events(&replay)?;
    Ok(replay)
}

/// Bookkeeping a caller can read without rasterizing: the outcome, the
/// counted gestures, the reconstructed active stroke count and the provenance
/// flags that decide whether a replay may be scored at all.
pub fn summary(r: &Replay) -> Summary {
    let active = replay_stacks(r).map_or(0, |stacks| stacks.active.len());
    Summary {
        is_final: r.outcome.is_some(),
        outcome: r.outcome,
        modifying_gestures: r.modifying_gestures,
        active_strokes: active,
        human_origin: r.human_origin,
        scoreable: r.scoreable,
        elapsed_ms: r.elapsed_ms,
    }
}

/// The categorical label plane the replay's active strokes describe: an
/// all-128 (Unknown) base with each applied stroke rasterized in order by
/// `trimap_strokes`, which owns the capsule geometry and the visit cap.
/// A refusal from that constructor is prefixed `rasterizer_limit: `.
pub fn label_plane(
    r: &Replay,
    width: u32,
    height: u32,
    control: Option<&DrJobControl>,
) -> Result<Vec<u8>, String> {
    if width != r.width || height != r.height {
        return Err("label plane dimensions do not match the replay".into());
    }
    let n = trimap_strokes::pixel_count(width, height)?;
    let stacks = replay_stacks(r)?;
    let mut strokes = Vec::with_capacity(stacks.active.len());
    for &position in &stacks.active {
        let event = &r.events[position];
        let (tool, diameter, points) = match (event.tool, event.diameter, event.points.as_ref()) {
            (Some(tool), Some(diameter), Some(points)) => (tool, diameter, points),
            _ => return Err("an applied stroke is missing its geometry".into()),
        };
        strokes.push(json!({
            "label": tool.label(),
            "radius": f64::from(diameter) / 2.0,
            "points": points.iter()
                .map(|p| [p[0] as f64 + 0.5, p[1] as f64 + 0.5])
                .collect::<Vec<_>>(),
        }));
    }
    let payload =
        json!({"version": 1, "width": width, "height": height, "strokes": strokes}).to_string();
    let _run = control.map(DrJobControl::begin).transpose()?;
    let job = control.map_or(JobContext::NONE, JobContext::new);
    let base = vec![128u8; n];
    trimap_strokes::apply_strokes(&base, width, height, &payload, job)
        .map_err(|e| format!("rasterizer_limit: {e}"))
}
