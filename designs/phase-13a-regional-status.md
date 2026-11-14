# Phase 13A — Regional evaluation (ADR)

- **Status:** Closed 2026-11-12. Every run has shipped: the regional and preview-divisor compositor at 8, 16 and 32 bits against a full-frame reference, measured cancellation in preparation, the bounded selection, artwork and stroke caps, and 13A-latency, which brought the 512×512 regional render back under its 13A-1 time and stated the divisor-8 preview's residual with its cause (`## Measured latency`). Carryover [P7-31](phase-7-carryovers.md) closed with it. What 13A handed on belongs to other phases: 13B-7's canvas display and packaged-base guided member ([#9](https://github.com/kpgalligan/doctored/issues/9)), 13B-9's sampling-plate items ([#10](https://github.com/kpgalligan/doctored/issues/10)) and 13D's typed latency and caps ([#11](https://github.com/kpgalligan/doctored/issues/11)); `## Open items` lists them.
- **Section:** **13A** of the [implementation plan](professional-photo-editor-implementation-plan.md) ("13. Scale to large production documents").
- **Originals:** [archive/2026-10/phase-13a-regional-status.md](../archive/2026-10/phase-13a-regional-status.md) — the doc as of 2026-10-07, with the full plan table, every contract delta, all 102 decisions, the verification records with their tables, and the struck open items ("the archived original" below); [archive/phase-13a-regional-status.md](../archive/phase-13a-regional-status.md) and [archive/phase-13a-regional-shipped.md](../archive/phase-13a-regional-shipped.md) — the status doc and its shipped companion at close.
- **Contract:** the module doc of `core/src/regional.rs` ("`regional.rs`" below); `core/MODULES.md`'s `regional.rs` entry lists the files (grep it, never read it whole).
- **Receipts:** `~/devel/doctored-fixtures/phase13-receipts/13a/`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Sub-slice plan

No run is left: 13A-latency, the last, shipped 2026-11-12. The typed regional path's latency is 13D's ([#11](https://github.com/kpgalligan/doctored/issues/11), `## Open items`).

## Shipped runs

The 33 shipped runs are listed with their commits under `## Runs`; each run's contract deltas follow, then the decisions in force (`## Decisions`) and the drives (`## Verification records`).

## Runs

The `Owns` / `Exit` cells and split lineage are in the archived original; 13A-latency's are `## Measured latency`.

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
| 13A-latency | the #9 batch (2026-11-12) | 11-12 |

## 13A-1 contract

Shipped: the regional / preview-divisor compositor at 8, 16 and float bits for flat stacks of rasters and pointwise adjustments, with its plan (`contract_version` 1), FFI and MCP `render` `region` / `preview_divisor`. Home: `regional.rs` module doc; plan JSON in `core/src/regional_plan.rs`. No full-frame fallback: a construct is admitted or refused by name.

## 13A-2a contract deltas

Shipped: layer masks (feather as a declared `Spatial` halo), clip units and typed Blend If. Home: `regional.rs` (UNITS, REFUSAL ORDER). A run of only hidden members is non-empty at 8 bits and empty at typed depth, as in each reference.

## 13A-2b contract deltas

Shipped: byte Blend If and fill opacity (a style with no enabled effect). Home: `regional.rs` (UNITS). A visible zero-opacity member is a clamp-only step: no plan row, but it still clamps a styled base's alpha.

## 13A-3a contract deltas

Shipped: groups at every depth, isolated and pass-through, gated, nested to the depth cap. Home: `regional.rs` (GROUPS, SAMPLING). The byte sampler tests the base's contribution before `clip_run`; an empty-extent group renders nothing; the window is walked sample-major (the byte unstaged loop run-major within a row since 13A-latency).

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

Shipped: an active-edge sweep in `artwork_coverage::rasterize`, sharing `row_coverage` with the retained `reference_rasterize`. Home: `core/src/artwork_coverage.rs`. The sweep never charges above the full scan; the 64M aggregate was kept (`## Decisions`). Since 2026-11-14 the row body's pair stage is `core/src/artwork_pair_sweep.rs`: two edges that both span the row are crossed only when an inversion sweep over their x at the row's top and bottom reports them, and the stage polls every 4,096 pair tests.

## 13A-6c-1 contract deltas

Shipped: the brush field over the stroke's footprint with exact hard-dab classification (oracle `reference_rasterize`). Home: `core/src/brush_coverage.rs`. Caps and sentences unchanged; the whole-stroke dab model was kept (`## Decisions`).

## 13A-6c-2 contract deltas

Shipped (Swift): one `NativeBrushStroke.dabLimitRefusal` sentence, thrown where `SoftBrush.stampCenters` used to truncate; unversioned 16-bit typed point strokes go through `dr_brush_u16`. Contract delta: such a stroke can refuse at both core limits (`## Decisions`).

## 13A-7 contract deltas

Shipped: `scripts/verify_phase12e2b_placement.py` runs every gate in one owned instance and adds P11 (the parked `NSAnimation _runBlocking` thread count stays at its starting value over 200 worker-backed edits). The close of 13A and P7-31 waited for 13A-latency, and came with it on 2026-11-12.

## 13A-latency contract deltas

Shipped: the byte unstaged window loop and the sampling plate's tile rows walk RUN-MAJOR (`core/src/doc_regional_run.rs`): one row's run, unit by unit, a plain raster or adjustment unit (no clip run, style or staged guide) hoisted onto `LayerRaster::row_segments`, every other unit per sample; a D5 Background level stays per sample. Runs are cut at the multiples of 4096 root samples, so the checkpoints fire where the per-sample loop's did (`core/tests/regional_run_path.rs` pins them row by row). The 8-bit Levels memoizes `apply8`'s output half per code (`Codes8`, `core/src/adjust_levels.rs`), bit for bit. A later byte fast path keeps both rules: no unit reads another sample, and a run never crosses a checkpoint. The typed path is untouched.

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
- **The tree, not the tile loop (13A-5c-2b).** Rejected: tiling `composite_level_into`, which would need a per-tile D5 / Shallow snapshot origin rule, full-extent materialization of nested styled groups and index-rewriting substitution, a second compositor. The tree's cost is its cancel unit, one tile row of root samples (one run-major run since 13A-latency), declared as `c_tree_row` and measured, not gated.
- **D5 / Shallow snapshots stay a stated difference, with cause (13A-5c-3).** The budget bounds memory a nest holds; the regional path holds neither snapshot (per-sample one-element backdrops; where the staged path does hold `S` or a pass-through hold over `R_0`, `stage_scratch` charges it). A dry-run charge for an allocation that never happens would refuse renders that fit, the opposite of 13A's purpose, so regional admits a superset by design; `the_background_and_pass_through_snapshot_charges_are_a_stated_difference` (`core/tests/regional_byte_knockout.rs`) stays the pin.
- **The 64M render aggregate stays (13A-6b), superseded 2026-11-14.** `dr_artwork_render` was a pointwise pass over planes the host built at the artwork's whole extent, and only a per-component window could shrink that input. The window landed (`core/src/artwork_window.rs`): a coverage or intrinsic component may declare the pixels its plane covers, the host windows each path part to its box, and the bound is 64M samples per component with no whole-layer total.
- **The WHOLE-STROKE dab model stays (13A-6c-1).** One quantization after the complete stroke is RZBR v1's contract; a per-batch model would quantize per batch and change recorded replays. The 50,000-dab cap and `raw_local_coverage`'s 4,096 are 13D's ([#11](https://github.com/kpgalligan/doctored/issues/11)).
- **The work cap is a contract delta, stated not avoided (13A-6c-2, objection 1).** Routing unversioned 16-bit strokes through `dr_brush_u16` subjects them to `MAX_BRUSH_WORK` as well as the dab cap; the MCP catalog (`AgentCatalog+Precision.swift`) and `app/FILES.md` name both limits, and gates T4 / T4b of `scripts/verify_phase13a6c2_stroke_refusals.py` pin the core's verbatim sentence on a ≈801-dab stroke far inside the dab cap. 8-bit unversioned strokes are untouched.

## 13A-6 cap survey (recorded in 13A-6a)

The caps that still stand (line numbers and per-megapixel thresholds: archived original):
- Selection: none. The typed Modify Selection kinds, Refine and the source selections run under a job control with no work refusal (`MAX_WORK_UNITS` 2·10⁹ bounded them before; it is now only the interactive preview budget, `core/src/precision_render.rs`). The byte selection transform stays at `MAX_PIXELS`; the byte feather and smooth (`core/src/doc_select.rs`) are bounded as the typed ones are, by the selection's support and the 250 px radius (`selection_limits::MAX_SELECTION_RADIUS`).
- Artwork: `MAX_PATH_POINTS` 262,144, `MAX_PATH_EDGES` 524,288 (was 262,144), `MAX_SCAN_WORK` 256M (`core/src/artwork_coverage.rs`); 64M samples per component, each plane windowed (`artwork_render::MAX_COMPONENT_SAMPLES`, `artwork_window`), no whole-layer total; text 4,096 UTF-16 units (`app/Sources/ArtworkTextCapture.swift`, P7-34 / 13D, [#11](https://github.com/kpgalligan/doctored/issues/11)).
- Strokes: `MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 2·10⁹ (256M before; `core/src/brush_coverage.rs`), Smudge's own `MAX_SMUDGE_WORK` 256M; `MAX_RAW_LOCAL_DABS` 4,096 (`core/src/raw_local_coverage.rs`, 13D's, [#11](https://github.com/kpgalligan/doctored/issues/11)).

## Measured latency

The table 13A-latency was gated on. `cargo run --release --example phase13a_regional_latency -- --storage both` renders the Phase 4 corpus stacked as three rasters (the corpus, then copies at (37, 21) Multiply 0.6 and (−15, 9) Screen 0.5) under one Levels adjustment; medians of 3; every regional output is checked equal to the full projection's crop before its time is reported. `--storage contiguous` re-adopts every raster contiguous (`LayerRaster::shared`); `--loop N` repeats the regional calls for a profiler.

13A-1 baseline (2026-09-23, `68650dd`): at 6000×4000 (24 MP) the full projection 1048.1 ms, the 512×512 region 13.1 ms, the divisor-8 preview of the whole canvas 18.5 ms; at 8000×6000 (48 MP) 2115.4, 12.8 and 37.0 ms. 13A-5b-iii-c-1 measured the regression at ≈2.2× and bisected its largest step to `c214a1b` (13A-3a); the tables are in the archived original.

13A-latency (2026-11-12; Release; three rounds interleaving `68650dd`'s example built in the same session, the base `f4f93fb2` and the batch, no build running, 1-minute load 3.7–6.6; bound declared before measuring: each regional median within 1.10× the same round's `68650dd` figure). Ranges are the three rounds' medians, in ms:

| | `68650dd` | before (`f4f93fb2`) | after, tiled | after, contiguous |
|---|---|---|---|---|
| 24 MP region 512 | 12.5–12.7 | 36.4–36.5 | 10.9–11.0 | 10.8–10.9 |
| 24 MP preview ÷8 | 17.9–18.9 | 55.5–55.6 | 20.1–21.0 | 15.5–15.8 |
| 48 MP region 512 | 12.4–12.6 | 36.0–37.4 | 10.7–10.9 | 10.8–11.0 |
| 48 MP preview ÷8 | 35.8–36.1 | 111.6–115.7 | 40.9–41.5 | 30.7–31.0 |
| 24 / 48 MP full projection | 1019–1024 / 2036–2052 | 1758–1762 / 3520–3562 | 941–1008 / 1892–1921 | 945–962 / 1871–1891 |

The region is inside the bound (0.85–0.87×, round by round). The tiled preview MISSES it (1.10–1.17× at 24 MP, one round just inside; 1.13–1.16× at 48 MP); the residual is the tiled storage, not the compositor: the same binary's contiguous twin is 0.82–0.88× of `68650dd`, and only a strided read sees it (region tiled / contiguous 0.98–1.01; preview 1.30–1.35), because a divisor-8 row reads 32 samples from each 256-pixel tile, three layers at a time, where a contiguous row is one stream. The cause of the 3× (`/usr/bin/sample` on `--loop 400`): about half the regional time was the per-sample tree dispatch (`ResolvedNode::pixel` / `composite`, `ResolvedUnit::sample`, `ResolvedLevel::sample`), about 28 % the byte Levels' 8-bit form (`LevelsStage::apply8`'s f64 code arithmetic and `powf`, which also took the full projection from ≈1020 to ≈1760 ms), and 3 % the per-sample tile reads. The run-major walk alone gives 20.6–21.1 / 34.5–34.6 ms (24 MP region / preview) and the Levels memo alone 25.1 / 39.3–39.6; both are needed.

The tree plate against the chain, `make cancel-bench`'s calibrations (`a_concurrently_cancelled_tree_plate_returns_within_one_tree_row`, `a_concurrently_cancelled_fallback_returns_within_one_chain_row`), back to back in three rounds at load 8–14: before, the tree T(1) / T(6) 397–404 / 932–944 ms and the chain 407–412 / 565–573 ms (T_tree(6) / T_chain(6) ≈ 1.65); after, the tree 264–266 / 840–848 ms, the chain unchanged. Under the issue's 3× at equal load, so nothing further was built.

The chain's figures are not comparable with 13A-5c-2a's 114.6 / 283.3 ms: that record measured a bare isolated group, and since 13A-5c-2b the fallback bench has put the group under a guided Shadows/Highlights (the one form the fallback still takes), which printed 400.9 / 564.6 ms the day it changed (2026-09-24, load ≈ 40). Re-measured on a quiet machine on 2026-11-13 (`python3 scripts/run_test_file.py regional_sampling_fallback_cancel a_concurrently_cancelled_fallback --ignored --test-threads=1 --nocapture`, five rounds, Release, no build running, 1-minute load 2.7–9.1): T(1) 398.4–415.0, T(6) 565.2–575.6 ms, PASS each, and `make cancel-bench` whole 411.2 / 574.9 ms; T(6) − T(1) 158–169 ms against 13A-5c-2a's 168.7, so the per-layer cost is unchanged. The bare Background under the same adjustment (`calibrated(&|| guided_over(&background(BW, BH)))`, added to the bench for one run) rendered in 337.2 ms (load 5.1), where that run's T(1) was 408.0: the constant is the guided adjustment, not a regression, and no code changed.

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

The 13A-5 runs (cancellation, progress, sampling plates and their accounting) and 13A-latency had no drive; the core suites (13A-latency's `core/tests/regional_run_path.rs`), the latency example and `make cancel-bench` verify them. 13A-5b-iii-c-1 measured and bisected the latency regression, and 13A-latency fixed it or stated its residual (`## Measured latency`, which also holds the tree plate's equal-load T(1) / T(6)). The `scripts/verify_phase9h_ui.py` activation failures in four records were a locked screen (the 13B status's `DROPPED` item).

## Open items

Verified 2026-10-07; fates set 2026-10-30; closed 2026-11-12. One bullet per standing row of the archived original (owners 13A-latency, 13D and unassigned); its 65 struck rows are history. The pair-term bullet comes from the 13A-6b contract deltas. Every item still open belongs to another phase.

- `DONE` (2026-11-12) — owner 13A-latency — The ≈2.2× regional slowdown: the region is back under 13A-1 and the preview's residual is the tiled storage's strided reads; the tree plate is 1.65× the chain at equal load, under 3× (`## Measured latency`).
- Handed to 13B: the 16 items of the 13B status's table "The 16 items handed from 13A" ([phase-13b-tiled-status.md](phase-13b-tiled-status.md#the-16-items-handed-from-13a)), which owns them: item 1 (MCP `render`'s plan) shipped 2026-11-12, items 2–3 are 13B-7's ([#9](https://github.com/kpgalligan/doctored/issues/9)), 4–12 dropped with 13B-8, 13–16 13B-9 ([#10](https://github.com/kpgalligan/doctored/issues/10)); and the two owner-13B items (a plan-level cancellation declaration, tighter per-child progress figures), dropped there.
- `DROPPED` (2026-11-14) — owner 13D — `MAX_WORK_UNITS` on the typed regional plan's `work_units`: the premise is gone. `precision_render::collect` no longer refuses past it (the constant is only the interactive preview budget, `designs/working-limit-ram-scaled.md`), and the regional render runs under its caller's control; refusing there would make the region refuse a document the full render admits.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — Typed (16-bit / float) regional latency is not measured at 24/48 MP.
- `DONE` (2026-11-14) — owner 13D — `artwork_render`'s 64M aggregate: a coverage or intrinsic component (RGB, float and CMYK) declares an optional `window`, outside which it reads 0, and the bound is 64M per component (`core/src/artwork_window.rs`); `NativeArtworkEvaluation` windows each path part to its box. Checks: `make t F=artwork_windows`; a 64-run text layer past the old total renders, alpha equal to a one-colour twin.
- `OPEN` [#11](https://github.com/kpgalligan/doctored/issues/11) — owner 13D — The 50,000-dab cap (`MAX_BRUSH_DABS`) and `raw_local_coverage`'s 4,096 dabs: any cap number (targets).
- `DONE` (2026-11-14) — owner 13D — The byte feather and smooth run over the selection's support grown by the Gaussian's half width (the typed path's `precision_selection_support`, made generic), equal to the whole plane byte for byte, and refuse a radius above 250 as the typed ones do. No numeric work refusal: the typed path has none. Check: `make t F=bounded_byte_feather`.
- `DONE` (2026-11-14) — owner 13D — The artwork scan's pair term: full × full pairs come from an inversion sweep (`core/src/artwork_pair_sweep.rs`) and are charged only when they cross, so the 18,000-edge 1×1 parallel outline finishes on the sweep (the reference still refuses) and a fan of crossing edges still refuses; the pair loops poll every 4,096 pair tests. Check: `make t F=bounded_artwork_coverage`.
- `DONE` (2026-11-12) — The load-sensitive default tests the September records called known flakes: `precision_layer_blending`, `refinement_cutout`, `agent_tests`, `phase8e_coverage`, `metadata_tests`, `bayesian_matting`. Their cancel tests park the worker (`common::probe::cancelled_parked_at`) and their timed bodies are `make cancel-bench` twins (build-velocity Phase 10); `agent_tests` binds a Unix socket in its own folder. The five suite binaries that mount them, run unfiltered and all five at once for ten rounds beside two other lanes' builds (1-minute load average 161 to 218 on 12 cores), passed 50 of 50 runs. `content_aware_*` runs alone (`SOLO` in `scripts/run_tests.py`).
- `DROPPED` (2026-10-30) — owner 13D — Selection Transform (`taps²·n`), source selections (`40n`, charged by the selection `plan` in `core/src/precision_selection.rs`), the typed layer-mask feather check (`precision_mask.rs`) and Blend (`32n`) staying canvas-bounded: they read the whole canvas by nature, so there is nothing to build.
