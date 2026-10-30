# Phase 9I — assisted annotation session runbook

**Status: deferred at the owner's discretion (2026-09-16).** This is the one step
of the 9I recovery that needs a human: a blinded, budgeted stroke session over
the declared 36-case subset of [gate v2](phase-9i-gate-v2-status.md). Nothing
else in the plan waits on it. Everything below is already built and verified;
the tooling is described in the [workspace note](phase-9i-assisted-workspace-status.md)
and the [scoring note](phase-9i-assisted-scoring-status.md), and the rules in
[protocol v1](phase-9i-assisted-protocol-v1.md). This page is only the order of
operations.

## What it costs and what it decides

- 36 cases: all 24 photographic crops plus 12 full images. Each case has a
  budget of **300 seconds and 20 modifying gestures** (strokes, Undo, Redo;
  a stroke the rasterizer refuses still counts). Worst case is three hours;
  most cases will take far less.
- It can be done **in sittings**. Each finished case locks independently, and
  reopening the same workspace later offers the next pending case. Do not
  export a second workspace for the same run: the protocol allows one attempt
  per case, and the scorer scores one workspace.
- If the scored run passes gate v2, 9I, P7-12 and the original Phase 6 gate
  close and the advanced refine controls can be enabled. If it fails, the exact
  per-case failures are recorded and the gate stays open. Either outcome is a
  valid end to the milestone; the decision table is in the
  [execution plan](phase-9i-next-steps.md).

## Before starting

- Do not look at reference planes, published cutout originals, candidate
  outputs, score reports or error maps for these fixtures. The task card inside
  the workspace is the only instruction the annotator reads.
- If you have already inspected any case, list its id with `--exposed` at
  export time. Exposed cases are scored and reported separately; they are not
  dropped.
- One annotation session at a time; the window refuses a second.

## Step 1 — build

The app and the two example binaries must be current:

```sh
make app
cd core && cargo build --release --example phase9i_assisted_workspace --example phase9i_assisted_quality && cd ..
```

## Step 2 — export a workspace

The output directory must not exist yet and must be outside the repository.
Both fixture directories are re-verified first; the corpus lives under
`~/devel/rasterize-fixtures/phase6/`.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase9i_assisted_workspace.py \
  ~/devel/rasterize-fixtures/phase9i-reports/assisted-run-1 \
  --executable core/target/release/examples/phase9i_assisted_workspace \
  --crops ~/devel/rasterize-fixtures/phase6/references \
  --full ~/devel/rasterize-fixtures/phase6/full-references \
  --annotator-id owner
```

Add `--exposed <case-id> …` for any case already inspected. The result is
`assisted-run-1/workspace/` (anonymous cases, task card, empty `replays/`) and
`assisted-run-1/private/` (the mapping the scorer needs; never open it during
annotation).

## Step 3 — annotate

Launch with the flag; in an ordinary launch the menu item does not exist:

```sh
RZ_ASSISTED_ANNOTATION=1 open build/Build/Products/Release/Rasterize.app
```

Then **File ▸ Open Assisted Annotation Case…** and choose the
`assisted-run-1/workspace` directory (the one containing `manifest.json`). The
window shows the task card, then the next pending case: the source image with
the starting prior. Paint **foreground**, **background** and **unknown**
strokes; there is no solver preview by design. The timer starts when the source
appears. When the budget runs out the case finalizes itself; closing the window
or quitting mid-case records the case as `incomplete` and locks it. Reopen the
workspace for the next case whenever convenient.

## Step 4 — score

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase9i_assisted_scoring.py \
  ~/devel/rasterize-fixtures/phase9i-reports/assisted-run-1-score \
  --workspace-root ~/devel/rasterize-fixtures/phase9i-reports/assisted-run-1 \
  --executable core/target/release/examples/phase9i_assisted_quality \
  --crops ~/devel/rasterize-fixtures/phase6/references \
  --full ~/devel/rasterize-fixtures/phase6/full-references
```

Exit code 0 means gate v2 passed on the full subset. The verdict is in
`run.json` (`numerical_gate_passed`, `assisted_result_claimed`,
`exposed_cases`, `missing_subset_cases`); per-case numbers are in
`matting/metrics.json` and `gate-rescore.json`. A run with missing cases is
scored but cannot claim completion.

## Step 5 — close out

Hand the score directory to Claude. On a pass, the closing litecode run
enables the advanced controls, records the assisted capability, and closes 9I,
P7-12 and Phase 6. On a fail, it records the per-case failures in a status doc
and the gate stays open with the reason.
