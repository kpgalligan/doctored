# Phase 13A — Regional evaluation: shipped runs

The settled record of Phase 13A's shipped runs, moved from [phase-13a-regional-status.md](phase-13a-regional-status.md) (condensed 2026-10-30; the verbatim text is in the archived original). The status doc keeps the state, the plan, the cap survey, `## Measured latency` and `## Open items`; "archived original", the receipts and "`regional.rs`" are as its head defines them.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Runs

The 31 shipped runs (`Owns` / `Exit` cells and split lineage: archived original):

| Run | Commit | Implemented (2026) |
|---|---|---|
| 13A-1 | `68650dd` | 09-23 |
| 13A-2a | `b7dd168` | 09-23 |
| 13A-2b | `e597a6d` | 09-23 |
| 13A-3a | `c214a1b` | 09-23 |
| 13A-3b-i | `6c9d050` | 09-23 |
| 13A-3b-ii | `8dbb083` | 09-23 |
| 13A-3c | `f859999` | 09-23 |
| 13A-4a | `32bfe66` | 09-23 |
| 13A-4b-i | `00397ec` | 09-23 |
| 13A-4b-ii-a | `843928e` | 09-23 |
| 13A-4b-ii-b-1 | `a5f2965` | 09-23 |
| 13A-4b-ii-b-2 | `0e39665` | 09-24 |
| 13A-5a | `1e3cf4e` | 09-24 |
| 13A-5b-i | `b9f7d75` | 09-24 |
| 13A-5b-ii-a | `fdad62c` | 09-24 |
| 13A-5b-ii-b | `15d5000` | 09-24 |
| 13A-5b-iii-a | `76a9aea` | 09-24 |
| 13A-5b-iii-b-1 | `5d19604` | 09-24 |
| 13A-5b-iii-b-2 | `057cbf5` | 09-24 |
| 13A-5b-iii-b-3 | `08aa01e` | 09-24 |
| 13A-5b-iii-c-1 | `d605faf` | 09-24 |
| 13A-5b-iii-c-2 | `dac3bf3` | 09-24 |
| 13A-5c-1 | `9683047` | 09-24 |
| 13A-5c-2a | `2704234` | 09-24 |
| 13A-5c-2b | `e47ace0` | 09-24 |
| 13A-5c-3 | `39f6a17` | 09-24 |
| 13A-6a | `58d4dbb` | 09-24 |
| 13A-6b | `b511129` | 09-24 |
| 13A-6c-1 | `49c8401` | 09-24 |
| 13A-6c-2 | `ab4664e` | 09-25 |
| 13A-7 | `576f69f` | 09-25 |

## 13A-1 contract

Shipped: the regional / preview-divisor compositor at 8, 16 and float bits for flat stacks of rasters and pointwise adjustments, with its plan (`contract_version` 1), FFI and MCP `render` `region` / `preview_divisor`. Home: `regional.rs` module doc; plan JSON in `core/src/regional_plan.rs`. No full-frame fallback: a construct is admitted or refused by name.

## 13A-2a contract deltas

Shipped: layer masks (feather as a declared `Spatial` halo), clip units and typed Blend If. Home: `regional.rs` (UNITS, REFUSAL ORDER). A run of only hidden members is non-empty at 8 bits and empty at typed depth, as in each reference.

## 13A-2b contract deltas

Shipped: byte Blend If and fill opacity (a style with no enabled effect). Home: `regional.rs` (UNITS). A visible zero-opacity member is a clamp-only step: no plan row, but it still clamps a styled base's alpha.

## 13A-3a contract deltas

Shipped: groups at every depth, isolated and pass-through, gated, nested to the depth cap. Home: `regional.rs` (GROUPS, SAMPLING). The byte sampler tests the base's contribution before `clip_run`; an empty-extent group renders nothing; the window is walked sample-major.

## 13A-3b-i contract deltas

Shipped: every layer style on a raster at 8 and 16 bits. Home: `regional.rs` (STYLES). An `"effects"` row declares `Global { "layer_style_planes" }` and the whole canvas as input, since plane rendering reads the whole layer; float carries no style.

## 13A-3b-ii contract deltas

Shipped: every layer style on an isolated group, by materialization (rendered before the loop as the reference renders it). Home: `regional.rs` (MATERIALIZATION); budget dry run in `core/src/doc_regional_budget.rs`. Plan and render refuse in the reference's own budget and nesting sentences.

## 13A-3c contract deltas

Shipped: knockout and "Blend Interior Effects as Group" through a per-sample unit pass (`core/src/regional_knockout.rs`). Home: `regional.rs` (KNOCKOUT, STATED DIFFERENCES). A knockout reads the same sample, so no plan field; D5 / Shallow snapshots are not charged (`## Decisions`).

## 13A-4a contract deltas

Shipped: a guide-reading Shadows/Highlights at the root through staged halo planes, exact, declared in the plan (`stages`, `border_policy`). Home: `regional.rs` (GUIDED SHADOWS/HIGHLIGHTS); geometry in `core/src/regional_guide.rs`. A preview below a guided row still runs its stages at divisor 1.

## 13A-4b-i contract deltas

Shipped: a guided Shadows/Highlights as a lone unit of an isolated group's level (staged per level at the level's buffer) and anywhere inside materialized subtrees (no stage). Home: `regional.rs` (NESTED GUIDES). The buffer is the group's rect at 8 bits and the canvas at typed depth.

## 13A-4b-ii-a contract deltas

Shipped: a guided Shadows/Highlights directly inside pass-through levels, staged by the enclosing level. Home: `regional.rs` (PASS-THROUGH GUIDES). A pass-through level is absorbed, never staged on its own; each held group charges one plane over `R_0`.

## 13A-4b-ii-b-1 contract deltas

Shipped: a guided clip member of an unstyled base, the unit re-run from a held "began" sample; plan types moved to `regional_plan.rs`. Home: `regional.rs` (CLIP-MEMBER GUIDES). A member stage's rows count from its unit's first row.

## 13A-4b-ii-b-2 contract deltas

Shipped: the same guided member under a styled base, including the typed package. Home: `regional.rs` (STYLED-BASE MEMBER GUIDES). The 8-bit package stays refused by name (`## Decisions`; 13B handed item 3, [#9](https://github.com/kpgalligan/doctored/issues/9)).

## 13A-5a contract deltas

Shipped: the cancellation model (interval `W_s` per stage, one return floor `T_f`) and checkpoints inside the byte mask feather and guide blur. Home: `regional.rs` (CANCELLATION MODEL). A bound is declared before it is measured and never widened; a cancelled blur caches nothing.

## 13A-5b-i contract deltas

Shipped: checkpoints inside the layer-style plane render at both depths; `style_plane_ops.rs` split out of `style_render.rs`. Home: `regional.rs` (CANCELLATION MODEL). A cancelled plane render caches nothing.

## 13A-5b-ii-a contract deltas

Shipped: `CompositeEnv::job` through the byte reference chain, live only from `resolve_materialized`; `DrJobControl::checkpoints`. Home: `regional.rs` (MATERIALIZATION, CANCELLATION MODEL). The job is `NONE` for every full-frame composite.

## 13A-5b-ii-b contract deltas

Shipped: per-row polling inside the style and knockout primitives; the probes moved to `core/tests/common/probe.rs`. Home: `regional.rs` (CANCELLATION MODEL). What stays one unit is bounded by `T_f + c_copy`.

## 13A-5b-iii-a contract deltas

Shipped: byte staged-path progress (the stage-0 and D5 snapshot fills report; the guide build reports inside its range). Home: `regional.rs` (CANCELLATION MODEL). One-unit bounds calibrate on the worker's own checkpoint `Timeline`.

## 13A-5b-iii-b-1 contract deltas

Shipped (tests, `Makefile`, docs): the load-robustness policy and `make cancel-bench`. Home: `core/CLAUDE.md` ("Load robustness") and `core/tests/common/probe.rs`; its residual is in `## Decisions`.

## 13A-5b-iii-b-2 contract deltas

Shipped: byte unstaged-path progress; the plan's `work_units` count the mask blur, style planes and materialization (`core/src/doc_regional_progress.rs`). Home: `regional.rs` (CANCELLATION MODEL). Preparation reports carry stage 1 and inner blurs stage 6, the entry the cancel suites read; typed reports stay stage 6 throughout (a stated difference).

## 13A-5b-iii-b-3 contract deltas

Shipped: the blocking checkpoint seam `DrJobControl::park_at` / `release` (`core/src/job_control.rs`, `#[doc(hidden)]`, armed by no FFI export). Home: `core/CLAUDE.md` ("Load robustness"). Default count-triggered cancel tests park the worker instead of racing a poller.

## 13A-5b-iii-c-1 contract deltas

Shipped: per-blur sub-ranges in `adjust_guide::build` (`GuideWork::blur_ranges`), equal per-effect sub-ranges, typed style planes inside their range. Home: `regional.rs` (CANCELLATION MODEL). Only progress reports moved. This run bisected the latency regression (`## Measured latency`).

## 13A-5b-iii-c-2 contract deltas

Shipped: the seven typed `JobContext::NONE` call sites pass the live job. Home: `regional.rs` (CANCELLATION MODEL). A cancel inside a typed materialization's styled composite returns within `T_f + c_row`.

## 13A-5c-1 contract deltas

Shipped: description / ICC preparation as a planned, polled stage (`adjust_calibrated::parse_scratch`, `validate_context_with_job`). Home: `regional.rs` (CANCELLATION MODEL). The interval is one meta parse; the typed render parses no description.

## 13A-5c-2a contract deltas

Shipped: the sampling plate's full-frame fallback made cancellable and its tiled path widened to masks, pointwise adjustments and opaque metadata. Home: `core/src/inpaint_source.rs` module doc. The plate reports no progress (the 13A-5c-2a decision, `## Decisions`).

## 13A-5c-2b contract deltas

Shipped: sampling plates with groups and styles through the byte regional tree given a `Cut` (`core/src/doc_regional_plate.rs`); `plate_arm` picks one of three arms. Every regional render passes `Cut::NONE`; a guided plate stays full-frame (13B handed item 13, [#10](https://github.com/kpgalligan/doctored/issues/10)).

## 13A-5c-3 contract deltas

Shipped: resource accounting: the parse scratch under every profile and the reference's uncharged per-unit buffers in each materialization's plan term. Home: `regional.rs` (STATED DIFFERENCES). Only `scratch_heap_bytes` moved.

## 13A-6a contract deltas

Shipped: typed Feather / Smooth / Grow / Shrink / Border and Refine evaluate over the selection's support, exactly (`core/src/precision_selection_support.rs`; oracle `reference_apply`). The work check moved from `plan` to `apply`, so `dr_selection_u16_plan` no longer refuses a beyond-cap spatial request.

## 13A-6b contract deltas

Shipped: an active-edge sweep in `artwork_coverage::rasterize`, sharing `row_coverage` with the retained `reference_rasterize`. Home: `core/src/artwork_coverage.rs`. The sweep never charges above the full scan; the 64M aggregate was kept (`## Decisions`).

## 13A-6c-1 contract deltas

Shipped: the brush field over the stroke's footprint with exact hard-dab classification (oracle `reference_rasterize`). Home: `core/src/brush_coverage.rs`. Caps and sentences unchanged; the whole-stroke dab model was kept (`## Decisions`).

## 13A-6c-2 contract deltas

Shipped (Swift): one `NativeBrushStroke.dabLimitRefusal` sentence, thrown where `SoftBrush.stampCenters` used to truncate; unversioned 16-bit typed point strokes go through `dr_brush_u16`. Contract delta: such a stroke can refuse at both core limits (`## Decisions`).

## 13A-7 contract deltas

Shipped: `scripts/verify_phase12e2b_placement.py` runs every gate in one owned instance and adds P11 (the parked `NSAnimation _runBlocking` thread count stays at its starting value over 200 worker-backed edits). The close of 13A and P7-31 moved to 13A-latency.

## Decisions

Only the decisions in force that code does not state. The others are stated by the files named above; the split lineage is in the archived original.

- **Retained (RAW) sources are baked at edit time (13A-4a decision, no code).** A binding's result is baked into `layer.pixels` / the typed record at EDIT time (`doc_retained_source.rs`), so the document compositor never evaluates it at render time and the regional path reads the same pixels exactly.
- **Global operators (13A-4a decision, no code).** No document-compositor operator performs a whole-frame pixel analysis: Auto Tone / Contrast / Color are commands (`ops_auto.rs`) and `selected_auto_levels.rs` is a destructive edit; the `document_v2` calibrated context is a description / ICC preparation already raised identically by plan and render (`validate_context`, 13A-1). Its scratch is charged since 13A-5c-1 (`adjust_calibrated::parse_scratch`, under every profile since 13A-5c-3).
- **Byte PACKAGE path — refuse by name, owner 13B-7 ([#9](https://github.com/kpgalligan/doctored/issues/9); 13A-4b-ii-b-1, narrowed by 13A-4b-ii-b-2).** `composite_package_into` (`core/src/style_composite.rs`) runs a packaged base's members per 256-row STRIP, so a member's guide is built from a strip-anchored, strip-clamped buffer no regional level has; staging it per strip is tiled-backing work (13B). A packaged base (`Styled::of`'s `package`: planes, opacity below 1, not Dissolve) refuses with `PACKAGED_BASE` in `core/src/regional.rs`, "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)"; the sentence names the package so a user can see why an effect-bearing base at full opacity renders and the same base at 0.6 does not. The TYPED package is one canvas-sized copy (`precision_style_render.rs`, no strips) and is admitted.
- **The cancellation model is per-stage (13A-5a).** A bound is `T_f + W_s · c_s`, never a sample count divided into a time that is mostly the fixed return floor: the hold fills' 19,900–91,000 "samples" misses of 13A-4b-ii-a … 13A-4b-ii-b-2 were the ≈0.01 ms floor over a 0.3 ns fill sample, not fill work. `T_f` = 1 ms is declared, not fitted (the hold-fill floors measured since 13A-4b-ii-a are 0.008–0.034 ms). The 13A-4a / 13A-4b-i printed sentences and `core/tests/regional_byte_guide.rs`'s declared bound are left as they were: still true, now looser, upper bounds; their records stand.
- **No plan field for the cancellation model (13A-5a).** The plan keeps `cancellation_checkpoint_pixels` only. MCP `render` passes no job control (`AgentServer.swift`), so a cancellation change moves nothing app-visible and owes no new MCP drive.
- **The load-robustness policy (13A-5b-iii-b-1)** is `core/CLAUDE.md`'s "Load robustness" rule (its Tests paragraph); the verbatim wording is in the archived original.
- **One residual in the polled bench and the INTERVAL guards (13A-5b-iii-b-1).** A poller stalled past the WHOLE render still fails, naming the stall: in `probe`'s entry wait (`core/tests/common/probe.rs`, "returned before the entry", the same code for every `Trigger`, so `After(0)` cannot remove it) or at the verdict ("completed before the cancel"). Such a failure is re-run serially by name and recorded, never chased. The default count-triggered tests park the worker instead (`DrJobControl::park_at`, `core/src/job_control.rs`, 13A-5b-iii-b-3); the polled `probe` stays for the bench and the INTERVAL guards.
- **The pre-cancelled `took` bound is printed, not asserted (13A-5b-iii-b-1),** at all four sites: the return floor is the bench's measurement, not a default-test verdict.
- **`assert_ramp`'s whole-range clause precedes its bounds clause (13A-5b-iii-b-1),** so a whole-range miss reports before any scan; it names a stall unambiguously only when the first value above the start is past the advance (`> end + 1e-6`); a first value AT the advance names both the stall and the `checkpoints_only` regression and demands a serial re-run. The bounds clause can still fail on a missed range END: bench-only, recorded.
- **The bench is `--no-fail-fast` (13A-5b-iii-b-1),** so one binary's panic never hides the others' record; its pass criterion is "every binary ran, every line recorded", not exit 0.
- **The plate still reports no progress (13A-5c-2a).** Every caller (`dr_doc_retouch_source`, `doc_inpaint`, `doc_content_aware` ×2, the promotion) runs it as a preparation whose work its own progress does not count, so both envs take `checkpoints_only()`; a per-tile / per-row report is 13B-9's (handed item 16, [#10](https://github.com/kpgalligan/doctored/issues/10)).
- **The tree, not the tile loop (13A-5c-2b).** Rejected: tiling `composite_level_into`, which would need a per-tile D5 / Shallow snapshot origin rule, full-extent materialization of nested styled groups and index-rewriting substitution, a second compositor. The tree's cost is the sample-major cancel unit, declared as `c_tree_row` and measured, not gated.
- **D5 / Shallow snapshots stay a stated difference, with cause (13A-5c-3).** The budget bounds memory a nest holds; the regional path holds neither snapshot (per-sample one-element backdrops; where the staged path does hold `S` or a pass-through hold over `R_0`, `stage_scratch` charges it). A dry-run charge for an allocation that never happens would refuse renders that fit, the opposite of 13A's purpose, so regional admits a superset by design; `the_background_and_pass_through_snapshot_charges_are_a_stated_difference` (`core/tests/regional_byte_knockout.rs`) stays the pin.
- **The 64M render aggregate stays: extent-bounded input, windowed components are 13D's ([#11](https://github.com/kpgalligan/doctored/issues/11); 13A-6b).** `dr_artwork_render` is a pointwise pass over planes the host already built at the artwork's own extent; the aggregate bounds that input, which regional evaluation cannot shrink. Only a per-component window in the contract could, and that is a new interface.
- **The WHOLE-STROKE dab model stays (13A-6c-1).** One quantization after the complete stroke is RZBR v1's contract; a per-batch model would quantize per batch and change recorded replays. The 50,000-dab cap and `raw_local_coverage`'s 4,096 are 13D's ([#11](https://github.com/kpgalligan/doctored/issues/11)).
- **The work cap is a contract delta, stated not avoided (13A-6c-2, objection 1).** Routing unversioned 16-bit strokes through `dr_brush_u16` subjects them to `MAX_BRUSH_WORK` as well as the dab cap; the MCP catalog (`AgentCatalog+Precision.swift`) and `app/FILES.md` name both limits, and gates T4 / T4b of `scripts/verify_phase13a6c2_stroke_refusals.py` pin the core's verbatim sentence on a ≈801-dab stroke far inside the dab cap. 8-bit unversioned strokes are untouched.

## Verification records

Receipts, tables and run logs of every run: the archived original. The drives, each gate's text being its script's docstring:

| Run | Drive | Gates |
|---|---|---|
| 13A-1 | `scripts/verify_phase13a1_regional.py` | R1–R5 |
| 13A-2a | `scripts/verify_phase13a2_masks_clips.py` | M1, M2, C1–C3, F1, B1, B2, N1 |
| 13A-2b | `scripts/verify_phase13a2b_byte_styles.py` | S1–S4 |
| 13A-3a | `scripts/verify_phase13a3_groups.py` | G1–G5 |
| 13A-3b-i | `scripts/verify_phase13a3b_styles.py` | E1–E4 |
| 13A-3b-ii | `scripts/verify_phase13a3bii_styled_groups.py` | SG1–SG4 |
| 13A-3c | `scripts/verify_phase13a3c_knockout.py` | K1–K4 |
| 13A-4a | `scripts/verify_phase13a4_guide.py` | G1–G5, F1 |
| 13A-4b-i | `scripts/verify_phase13a4b_nested_guide.py` | N1–N5, F1 |
| 13A-4b-ii-a | `scripts/verify_phase13a4bii_pass_through.py` | P1–P4, F1 |
| 13A-4b-ii-b-1 | `scripts/verify_phase13a4biib_clip_members.py` | C1–C4, F1 |
| 13A-4b-ii-b-2 | `scripts/verify_phase13a4biib2_styled_members.py` | S1–S5, F1 |
| 13A-6a | `scripts/verify_phase13a6a_bounded_selection.py` | B1–B4 |
| 13A-6b | `scripts/verify_phase13a6b_bounded_artwork.py` | B1–B3 |
| 13A-6c-1 | `scripts/verify_phase13a6c_bounded_strokes.py` | B1–B4 |
| 13A-6c-2 | `scripts/verify_phase13a6c2_stroke_refusals.py` | T1–T4, T4b, B1–B3, Q1, C1 |
| 13A-7 | `scripts/verify_phase12e2b_placement.py` | P1–P11 |

The 13A-5 runs (cancellation, progress, sampling plates and their accounting) had no drive; the core suites and `make cancel-bench` verify them. 13A-5b-iii-c-1 measured and bisected the latency regression (the status doc's `## Measured latency`). What still stands is an item of the status doc's `## Open items`: the latency regression and the tree plate's T(1) / T(6) at unequal load ([#9](https://github.com/kpgalligan/doctored/issues/9)), and the load flakes the records name ([#22](https://github.com/kpgalligan/doctored/issues/22)). The `scripts/verify_phase9h_ui.py` activation failures in four records were a locked screen (the 13B status's `DROPPED` item).
