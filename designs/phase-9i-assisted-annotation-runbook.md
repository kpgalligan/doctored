# Phase 9I assisted annotation session runbook (ADR)

- **Status:** Accepted — closed. The session was dropped on 2026-10-30 (the gate is no longer a ship condition); the steps below stay as the procedure should it ever be run.
- **Phase / dates:** 9I, 2026-09-16
- **Original:** [archive/phase-9i-assisted-annotation-runbook.md](../archive/phase-9i-assisted-annotation-runbook.md)
- **Key commits:** `81d6aa5`

## Context
The assisted protocol is measured against [gate v2](phase-9i-gate-v2-status.md) on its declared 36-case subset; the
run is no longer a ship condition. Everything is built and verified — the exporter and annotation window
([phase-9i-assisted-workspace-status.md](phase-9i-assisted-workspace-status.md)), the scorer
([phase-9i-assisted-scoring-status.md](phase-9i-assisted-scoring-status.md)) and the rules
([phase-9i-assisted-protocol-v1.md](phase-9i-assisted-protocol-v1.md)). This page is only the order of operations.

## Decisions
- **Cost:** 36 cases (24 photographic crops + 12 full images), each **300 s and 20 modifying gestures** (strokes,
  Undo, Redo; a rasterizer-refused stroke still counts). Worst case three hours.
- **Sittings are allowed:** each finished case locks independently; reopening the same workspace offers the next
  pending case. **Export only one workspace per run** — one attempt per case, and the scorer scores one workspace.
- **Blinding:** do not look at reference planes, published cutout originals, candidate outputs, score reports or
  error maps. The workspace's `TASK-CARD.md` is the only instruction the annotator reads. Already-inspected cases
  are declared with `--exposed` at export; they are scored and reported, never dropped.
- One annotation session at a time; the window refuses a second.

### Step 1 — build
```sh
make app
cd core && cargo build --release --example phase9i_assisted_workspace --example phase9i_assisted_quality && cd ..
```

### Step 2 — export a workspace
Output must not exist and must be outside the repository. Both fixture directories are re-verified first.
```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase9i_assisted_workspace.py \
  ~/devel/doctored-fixtures/phase9i-reports/assisted-run-1 \
  --executable core/target/release/examples/phase9i_assisted_workspace \
  --crops ~/devel/doctored-fixtures/phase6/references \
  --full ~/devel/doctored-fixtures/phase6/full-references \
  --annotator-id owner
```
Add `--exposed <case-id> …` for any case already inspected. Result: `assisted-run-1/workspace/` (anonymous cases,
task card, empty `replays/`) and `assisted-run-1/private/` (the mapping the scorer needs — never open it during annotation).

### Step 3 — annotate
```sh
DR_ASSISTED_ANNOTATION=1 open build/Build/Products/Release/Doctored.app
```
Without the flag the menu item does not exist (`AssistedAnnotation.isEnabled` in
[`app/Sources/AssistedAnnotation.swift`](../app/Sources/AssistedAnnotation.swift); the
`AssistedAnnotationEnabled` default also enables it). Then **File ▸ Open Assisted Annotation Case…** and choose
`assisted-run-1/workspace` (the directory containing `manifest.json`). The task card appears, then the next pending
case: source plus starting prior. Paint **foreground**, **background** and **unknown** strokes; there is no solver
preview by design. The timer starts when the source appears; the case finalizes itself when the budget runs out.
Closing the window or quitting mid-case records `incomplete` and locks the case.

Launch notes (verified against the current tree, not in the original): `open` passes the shell environment to a
*newly launched* app, but if a Doctored from the same bundle is already running it just activates that instance
without the flag — quit it first or use `open -n`. Do not have `DR_BACKGROUND=1` exported for this human session;
it would keep the window behind other apps.

### Step 4 — score
```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase9i_assisted_scoring.py \
  ~/devel/doctored-fixtures/phase9i-reports/assisted-run-1-score \
  --workspace-root ~/devel/doctored-fixtures/phase9i-reports/assisted-run-1 \
  --executable core/target/release/examples/phase9i_assisted_quality \
  --crops ~/devel/doctored-fixtures/phase6/references \
  --full ~/devel/doctored-fixtures/phase6/full-references
```
Exit 0 means gate v2 passed on the full subset. Verdict in `run.json` (`numerical_gate_passed`,
`assisted_result_claimed`, `exposed_cases`, `missing_subset_cases`); per-case numbers in `matting/metrics.json`
and `gate-rescore.json`. A run with missing cases is scored but cannot claim completion.

### Step 5 — close out
Hand the score directory to Claude. On a pass: record the assisted capability and close the gate-v2 measurement
(the refine controls already ship, since 2026-10-11). On a fail: record the per-case failures in a status doc with
the reason. Either outcome validly ends the milestone (decision table in [phase-9i-next-steps.md](phase-9i-next-steps.md)).

## Consequences
- The runbook's commands are coupled to the scripts' argument names and to `designs/phase-9i-assisted-annotation-task-v1.md`,
  which the exporter copies byte-for-byte into the workspace as `TASK-CARD.md`.
- A pass on its own does not qualify a typed estimator or public workflow.

## Open items (verified 2026-09-26)
- `DROPPED` (2026-10-30) — Run the session and score it: the matting gate stopped being a ship condition on 2026-10-10 and P7-12's other half shipped without it, so it gates nothing ([gate v2](phase-9i-gate-v2-status.md)).
- `DROPPED` (2026-10-30) — After a pass, typed estimator qualification and public integration of the refine controls: no pass is coming, and the typed estimator and the Select and Mask UI/MCP/Actions outputs at 8 and 16 bits shipped 2026-10-11 without the gate ([typed cutouts](phase-9i-typed-cutouts-status.md)).
