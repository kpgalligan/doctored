# Phase 13A — Regional evaluation: status

Section **13A** of the [implementation plan](professional-photo-editor-implementation-plan.md)
("## 13. Scale to large production documents"), carryover
[P7-31](phase-7-carryovers.md). 13A is split into seven bounded runs, delivered
in series; this document is the plan every later run picks up and the record of
each run. **13A-1 is Implemented 2026-09-23. 13A-2 was SPLIT into 13A-2a and
13A-2b; 13A-2a and 13A-2b are Implemented 2026-09-23. 13A-3 was SPLIT into
13A-3a, 13A-3b and 13A-3c; 13A-3a is Implemented 2026-09-23. 13A-3b was SPLIT
into 13A-3b-i and 13A-3b-ii; 13A-3b-i, 13A-3b-ii and 13A-3c are Implemented
2026-09-23. 13A-4 was SPLIT into 13A-4a and 13A-4b; 13A-4a is Implemented
2026-09-23. 13A-4b was SPLIT into 13A-4b-i and 13A-4b-ii; 13A-4b-i is
Implemented 2026-09-23. 13A-4b-ii was SPLIT into 13A-4b-ii-a and 13A-4b-ii-b;
13A-4b-ii-a is Implemented 2026-09-23. 13A-4b-ii-b was SPLIT into
13A-4b-ii-b-1 and 13A-4b-ii-b-2; 13A-4b-ii-b-1 is Implemented 2026-09-23;
13A-4b-ii-b-2 is Implemented 2026-09-24. 13A-5 was SPLIT into 13A-5a, 13A-5b
and 13A-5c; 13A-5a is Implemented 2026-09-24. 13A-5b was SPLIT into
13A-5b-i, 13A-5b-ii and 13A-5b-iii; 13A-5b-i is Implemented 2026-09-24.
13A-5b-ii was SPLIT into 13A-5b-ii-a and 13A-5b-ii-b; 13A-5b-ii-a and
13A-5b-ii-b are Implemented 2026-09-24. 13A-5b-iii was SPLIT into
13A-5b-iii-a, 13A-5b-iii-b and 13A-5b-iii-c; 13A-5b-iii-a is Implemented
2026-09-24. 13A-5b-iii-b was SPLIT into 13A-5b-iii-b-1 and 13A-5b-iii-b-2;
13A-5b-iii-b-1 and 13A-5b-iii-b-2 are Implemented 2026-09-24; 13A-5b-iii-b-2
DECIDED the blocking checkpoint seam and deferred it to a new sub-slice
13A-5b-iii-b-3; 13A-5b-iii-b-3 is Implemented 2026-09-24. 13A-5b-iii-c was
SPLIT into 13A-5b-iii-c-1 and 13A-5b-iii-c-2; 13A-5b-iii-c-1 and
13A-5b-iii-c-2 are Implemented 2026-09-24. 13A-5c was SPLIT into 13A-5c-1,
13A-5c-2 and 13A-5c-3; 13A-5c-1 is Implemented 2026-09-24. 13A-5c-2 was
SPLIT into 13A-5c-2a and 13A-5c-2b; 13A-5c-2a and 13A-5c-2b are Implemented
2026-09-24; 13A-5c-3 is Implemented 2026-09-24. 13A-6 was SPLIT into
13A-6a, 13A-6b and 13A-6c; 13A-6a and 13A-6b are Implemented
2026-09-24. 13A-6c was SPLIT into 13A-6c-1 and 13A-6c-2; 13A-6c-1 is
Implemented 2026-09-24; 13A-6c-2 is Implemented 2026-09-25. 13A-7 is
Implemented 2026-09-25.** Every 13A sub-slice EXCEPT the deferred
**13A-latency** is implemented. **13A is NOT closed, and neither is
[P7-31](phase-7-carryovers.md):** both close when 13A-latency lands (or its
residual is stated with cause); the open items still standing are listed
with their owners (13B, 13D, 13A-latency) under Open items, so the
close-out is mechanical.

## Sub-slice plan (recorded verbatim from the 13A-1 spec)

| Run | Owns | Exit |
|---|---|---|
| **13A-1** | Regional/preview-divisor DOCUMENT compositor at 8/16/float for unmasked, unclipped, unstyled top-level rasters + pointwise adjustments; every other construct REFUSED by name; per-layer evaluation-need declaration in the plan; full-frame reference untouched; FFI + Swift + MCP `render` `region`/`preview_divisor`; drive | regional == reference sample-for-sample at all depths; full-frame byte-identical; measured 24/48 MP latency |
| 13A-2 | Layer masks (raw placement, density, feather = declared halo), clip runs (incl. hidden members and baseless clipped entries, which 13A-1 over-refuses), Blend If | same oracle incl. mask feather at region edges |
| **13A-2a** (split from 13A-2) | Layer masks at both depths (raw placement, density, feather = declared `Spatial` halo, outside coverage, enabled or not), clip units at both depths (hidden and zero-opacity members, hidden bases, adjustment bases, baseless clipped entries), TYPED Blend If (lone raster, clip base, member); the three 13A-1 review nits | same oracle incl. mask feather at region and canvas edges |
| **13A-2b** (split from 13A-2) | BYTE Blend If and fill opacity — a style with no enabled effects, which reaches `composite_direct_into` → `composite_pixels_into` (`BackdropSnapshot`) and, on a clip base, `Shape::of_layer` + `clamp_alpha_to_shape`; still refused "carries a layer style" | same oracle |
| 13A-3 | Isolated/pass-through groups, styles (pad = halo), knockout, nested budget | same oracle incl. group effects |
| **13A-3a** (split from 13A-3) | GROUPS at both depths: isolated and pass-through groups (plain and gated by opacity/mask, nested to the depth cap, as lone entries, clip bases, clip members, baseless clipped entries, hidden, empty) admitted regionally at 8, 16 and float; the "is a group" / "is inside a group" refusals removed; the two 13A-2b review nits | same oracle, sample for sample, at 8, 16 and float |
| 13A-3b (split from 13A-3) | Layer styles with ENABLED effects at 8 bits (planes from `LayerStyle::rendered`, `Spatial { halo: pad + shadow shift }`), styled groups with effects, EVERY typed style (fill-only included: extract `precision_style_render::composite_pixels_into`'s body and `typed_shape`'s coverage byte); pad = halo | same oracle incl. group effects |
| **13A-3b-i** (split from 13A-3b) | Every layer style on a RASTER at 8 and 16 bits — enabled effects (planes from `LayerStyle::rendered` / `typed_shape` + `render_effects`, the reference's own preparation), fill opacity, Blend If, the package model — lone, clip base, clip member, inside groups; a TYPED effect-less (fill / Blend If) style on an isolated group; an `"effects"` row declares `Global { analysis: "layer_style_planes" }` (the plan's input is the whole canvas); the new refusals "is a group with layer-style effects" and "blends interior effects as a group"; the three 13A-3a review nits | same oracle, sample for sample, at 8 and 16 bits |
| 13A-3b-ii (split from 13A-3b) | Styled GROUPS with enabled effects at both depths (8 bits: `doc_group::rendered_group`'s materialized synthetic raster over the extent; typed: `render_styled`'s canvas-sized children buffer) and the `MAX_GROUP_BUFFER_PIXELS` / `RenderBudget` counterpart that comes with materialization | same oracle incl. group effects |
| **13A-3c** (split from 13A-3) | Knockout (D4/D5 Background snapshot, member snapshots) and `blend_interior_as_group` with grouped interiors at both depths | same oracle |
| 13A-4 | Spatial neighbourhoods (Shadows/Highlights guide, live-filter stack) with declared halo/border policy; GLOBAL operators (`document_v2` calibrated context, Auto analysis) declare analysis scratch + evaluation | plan declares every need; oracle within a tolerance fixed per operator before measuring |
| **13A-4a** (split from 13A-4) | A guide-reading Shadows/Highlights at the ROOT (lone or baseless clipped) at 8 and 16 bits through STAGED halo planes (`regional_guide`), declared in the plan (`stages`, `border_policy`, the composed input halo); the live-filter and global-operator DECISIONS (no code); the three 13A-3c review nits | same oracle, EXACT, at 8 and 16 bits; measured cancel-to-return |
| 13A-4b (split from 13A-4) | A guided Shadows/Highlights inside a GROUP (8 bits: the guide lattice anchored at the group's EXTENT buffer origin, `rendered_group`; typed: the canvas-sized buffer), as a CLIP MEMBER (the unit buffer) and inside MATERIALIZED subtrees — per-level stages; the deferred-placement refusal sentence is removed there | same oracle |
| **13A-4b-i** (split from 13A-4b) | A guided Shadows/Highlights as a lone unit of any ISOLATED group's level (nested to the cap, any unit position; the group lone, a clip base, a clip member, inside plain / gated pass-through groups, masked, styled without effects) through per-level stages anchored at the level's BUFFER (8 bits: the group's rect; typed: the canvas), and anywhere inside MATERIALIZED subtrees (the reference renders them, no stage); the two carried 13A-4a nits (trees below 600 lines, the 8-bit final-stage cancel calibration) | same oracle, EXACT, at 8 and 16 bits; measured cancel-to-return |
| 13A-4b-ii (split from 13A-4b) | A guided Shadows/Highlights as a CLIP MEMBER (the accumulator-aligned unit buffer, the base-only backdrop held as a plane) and directly inside PASS-THROUGH levels (the enclosing accumulator, the gated copy, the Shallow snapshot plane); the refusal sentence is removed there | same oracle |
| **13A-4b-ii-a** (split from 13A-4b-ii) | A guided Shadows/Highlights directly inside PASS-THROUGH levels (plain or gated by opacity / mask, nested, at the root or inside an isolated level) at 8 and 16 bits, staged through the ENCLOSING accumulator (the gated copy is the stage plane itself; the sample as the children began HELD as a plane); the two carried 13A-4b-i items (the cancel-bound wording in `regional.rs`, the empty-base member records) and the walk split | same oracle, EXACT, at 8 and 16 bits; measured cancel-to-return |
| 13A-4b-ii-b (split from 13A-4b-ii) | A guided Shadows/Highlights as a CLIP MEMBER — the accumulator-aligned unit buffer held as a plane (`doc.rs:576-596`), the base-only backdrop plane, the styled-direct (`style_composite.rs:422-436`) and routed (`style_knockout.rs:660-682`) member loops, and the PACKAGE path, whose `composite_package_into` (`style_composite.rs:479-540`) runs a packaged base's members per STRIP (decide refuse vs. per-strip there); the refusal sentence is removed there | same oracle |
| **13A-4b-ii-b-1** (split from 13A-4b-ii-b) | A guided Shadows/Highlights as a CLIP MEMBER of an UNSTYLED base (byte `composite_clip_group_into`'s private buffer, typed `render_layers`' `clipped_members` branch) at 8 and 16 bits — at any admitted level, inside pass-through groups, beside knockout members, under a typed Blend If base — staged through the enclosing level with the unit RE-RUN from a held "began" sample; a member stage's rows counted from the unit's first row; a styled base refuses "… under a styled base"; the three carried 13A-4b-ii-a items (the gated-hold `debug_assert!`, `regional.rs` below 600 lines, the PACKAGE decision) | same oracle, EXACT, at 8 and 16 bits; measured cancel-to-return |
| **13A-4b-ii-b-2** (split from 13A-4b-ii-b) | A guided member under a STYLED base — the styled-direct loop (`style_composite.rs:422-436`, typed `precision_style_render::composite_direct_into`) and the routed loop (`style_knockout.rs:660-682`, `precision_style_knockout`): the began hold taken as the unit BEGAN, the staged member index a STOP inside the style's own member loop (the private buffer left in the sample before `composite_source_into` / `K` and the Interior contributions), every later fill's FINISH re-running the full styled sample from the hold; the typed PACKAGE (one canvas-sized copy); the "under a styled base" sentence removed there (the byte PACKAGE path stays refused by name, owner 13B) | same oracle |
| 13A-5 | P7-31: full-frame legacy promotion/mask fallbacks and preparation stages (mask alignment/feather, ICC/description preparation, Sample All grouped-source fallback) to measured cancellation/progress | declared bound, measured cancel-to-return |
| **13A-5a** (split from 13A-5) | The cancellation MEASUREMENT MODEL (a per-stage checkpoint interval `W_s` and one return floor `T_f`; bound `T_f + W_s · c_s`); cancellation checkpoints inside the two byte preparation BLURS the regional render runs before its loops — the mask feather (`MaskProperties::derived_with_job`, a cancelled blur caching nothing) and the byte guide's blur (the u8 `GuideSample::blur`) — both under `JobContext::checkpoints_only`; the byte HOLD fill advancing progress; the three carried 13A-4b-ii-b-2 nits | declared model, measured cancel-to-return per stage; full-frame byte-identical |
| 13A-5b (split from 13A-5) | Byte `LayerStyle::rendered` / `rendered_group` and typed `render_effects` checkpoints; byte preparation PROGRESS (plan `work_units` counting the mask blur, planes, materialization; the stage-0 `prev` / D5 `snapshot` and unstaged accumulator fills reporting; the byte guide's blur reporting inside a range on the typed `precision_regional_guide` pattern) and the resulting re-basing of every byte probe table | declared bound, measured cancel-to-return |
| **13A-5b-i** (split from 13A-5b) | The carried reports-once GUARD fix (a load-independent report-log signature in `regional_byte_preparation_cancel.rs`) and cancellation checkpoints inside the LAYER-STYLE PLANE RENDER — byte `LayerStyle::rendered_with_job` (through `Styled::of`, a cancelled render caching nothing) and the typed `render_effects_with_job` (`prepare_style`, `composite_styled_into`), both under `checkpoints_only`: inside every blur, dilate and erode, per ROW inside every bespoke per-pixel loop, at the boundaries of every other primitive; `style_plane_ops.rs` split out of `style_render.rs` | declared bound, measured cancel-to-return; full-frame byte-identical; the guards proven under parallel load |
| 13A-5b-ii (split from 13A-5b) | Byte `rendered_group` checkpoints: a `JobContext` threaded through `doc_group::composite_level_into` / `composite_unit_into` / `composite_layer_into` (the byte reference compositor; `CompositeEnv` carries none today) | declared bound, measured cancel-to-return |
| **13A-5b-ii-a** (split from 13A-5b-ii) | `CompositeEnv::job` threaded through the byte reference chain (`rendered_group` → `composite_level_into` → `composite_unit_into` → `composite_clip_group_into` / `composite_layer_into` → `composite_styled_into`, and the knockout unit pass), live only from `resolve_materialized` (narrowed `checkpoints_only`), polled per ROW in the chain's own loops and at the boundaries of every other primitive; every child layer's enabled mask through `Layer::active_mask_with_job`; the carried 13A-5a probe fix (a checkpoint-count trigger, `RzJobControl::checkpoints`) | declared bound, measured cancel-to-return; full-frame byte-identical; both cancel suites proven under parallel load |
| **13A-5b-ii-b** (split from 13A-5b-ii) | Per-row polling inside the primitives 13A-5b-ii-a leaves ONE unit — `composite_pixels_into`, `composite_contribution`, `clamp_alpha_to_shape`, `composite_buffer_into`, `BackdropSnapshot::of`, the package strip mix, `encode_projection` / `quantize`, `Shape::of_layer`, the mask crops (`effective_mask_rect_with_job`'s `from_fn` and the aligned plane; a `job` parameter on functions that carry no env; 7 typed call sites pass `NONE`); 13A-5b-i's time probes in `regional_style_preparation_cancel.rs` moved onto the checkpoint trigger; the carried `composite_gated_pass` zero-width panic | declared bound, measured cancel-to-return |
| 13A-5b-iii (split from 13A-5b) | Byte preparation PROGRESS (plan `work_units` for the mask blur, planes, materialization; the stage-0 `prev` / D5 `snapshot` and unstaged accumulator fills reporting; the byte guide's blur reporting inside a range; typed style planes inside a range) and the re-basing of every byte probe table | declared bound, measured cancel-to-return |
| **13A-5b-iii-a** | The byte STAGED path's progress: the stage-0 `prev` and D5 `snapshot` fills advance and report (each `|R_0|` in the byte plan's `work_units` — the typed `fill`'s twin), the byte guide build reports INSIDE ITS RANGE on the typed pattern; the four carried test items (factor-8 and (m2) calibrations, the probe-helper fold, the three nits); the five byte guide probe tables and the preparation suite's guide probes re-based | declared bound, measured cancel-to-return; staged output unchanged |
| 13A-5b-iii-b | The byte UNSTAGED path's progress: the accumulator fill reporting AND the plan's `work_units` for the mask blur, style planes and materialization, each reporting inside its own range (a `Run` through `Level::resolve` / `Node::resolve` / `Styled::of` / `resolve_materialized`) — these land TOGETHER, because a reporting fill under `fetch_max` erases the stage-1→6 flip the four cancel suites' entries (`at_the_blur`, `inside_the_chain`) read, and only a per-stage range restores an observable entry; re-bases the four cancel suites and the seven non-guide byte cancel tests | declared bound, measured cancel-to-return |
| **13A-5b-iii-b-1** (split from 13A-5b-iii-b) | The orchestrator's LOAD-ROBUSTNESS policy for the four regional cancel suites: the default `make test` keeps only load-independent assertions; every wall-clock / short-lived-observation verdict (the five measured PASS/MISS and RAMP tests) moves behind `#[ignore]` and runs through a new `make cancel-bench` (release, `--no-fail-fast --ignored --test-threads=1`); the four pre-cancelled `took` bounds printed, not asserted; the style suite's byte cache-integrity test cancels `After(0)` at the blur's flip at every size; `assert_ramp`'s scan-start clause; the primitive and style suites' local report-log trio folded into `common::probe` | `make test` twice + 4× parallel rounds green; `make cancel-bench` once serially, every binary ran, PASS/MISS table and any panic recorded |
| **13A-5b-iii-b-2** (split from 13A-5b-iii-b) | The byte UNSTAGED path's progress: the accumulator fill reporting AND the plan's `work_units` for the mask blur, style planes and materialization, each reporting inside its own range (a `Run` through `Level::resolve` / `Node::resolve` / `Styled::of` / `resolve_materialized`) — these land TOGETHER, because a reporting fill under `fetch_max` erases the stage-1→6 flip the four cancel suites' entries (`at_the_blur`, `inside_the_chain`) read, and only a per-stage range restores an observable entry; re-bases the four cancel suites and the seven non-guide byte cancel tests; the staged path's `Level::resolve` shares the ranges; re-bases the four suites (default entries and bench probes), the seven non-guide byte cancel tests, and every byte test pinning `work_units` or a progress point on a fixture with a feathered mask, an effects style or a materialized group (known: `regional_byte_styles`, `regional_byte_effects`, `regional_byte_styled_groups`, `regional_byte_knockout`, `regional_byte_styled_clip_guide`, `regional_byte_nested_guide`; verify by grep) | declared bound, measured cancel-to-return |
| 13A-5b-iii-b-3 (decided in 13A-5b-iii-b-2) | The blocking checkpoint seam: `#[doc(hidden)] RzJobControl::park_at(count)` / `release()`, `checkpoint()` parking on a `Condvar` at that count; the four suites' default count-triggered tests take their entry count from the worker's own uncancelled state timeline; removes the 'returned before the entry' residual | `make test` twice + 4× parallel rounds green with the residual gone; `make cancel-bench` once serially |
| 13A-5b-iii-c | Per-blur SUB-RANGES inside `adjust_guide::build` (its two grid blurs each ramp 0→1 under one range, so progress pins at the range's end after the first — `build` is shared by the typed driver and the full-frame `tone_plane_with_job`, so the split moves typed progress too) and typed style planes inside a range (per-effect sub-ranges, the same defect); the 7 typed `JobContext::NONE` call sites (`precision_style_render.rs` ×5, `precision_style_knockout.rs` ×2 — `job` is already in scope in each) with a measured typed interval | declared bound, measured cancel-to-return |
| **13A-5b-iii-c-1** (split from 13A-5b-iii-c) | Per-blur SUB-RANGES inside `adjust_guide::build` (`adjust_precision_plan::GuideWork::blur_ranges`, the plan's work terms' one home; byte, typed, full-frame and destructive callers alike, full-frame output byte-identical); `render_effects_with_job`'s EQUAL per-effect sub-ranges; the typed regional style planes reported inside their range in `prepare_style` (`progress.job.range(start, end)`, no `checkpoints_only`); the typed style suite re-based (INTERVAL from the advance to the loop's first chunk, bench RAMP, `typed_window` / `typed_probe` exits and verdict); a new typed guide suite (`regional_typed_preparation_cancel.rs`, in `make cancel-bench`); the two carried nits (`cancelled_parked_at`'s worker releases on unwind; `regional.rs` reflow); the latency example re-measured with an interleaved A/B against `68650dd` and, the regression holding on a quiet machine, bisected (recorded, not fixed) | `make test` twice + 4× parallel rounds of the five default cancel suites green; `make cancel-bench` once serially, the new RAMPs passed |
| **13A-5b-iii-c-2** (split from 13A-5b-iii-c) | The 7 typed `JobContext::NONE` call sites of the shared primitives (`precision_style_render.rs:350,372,439,457,488`, `precision_style_knockout.rs:457,489` — `job` already in scope in each); `composite_styled_into` handed `render_styled`'s `progress.job.range(start, end)` with its `checkpoints_only` dropped; a measured typed interval inside a typed materialization's styled composite | declared bound, measured cancel-to-return |
| **13A-latency** (DEFERRED — litecode run `phase-13a-regional-latency-regression`, request and triage written 2026-09-24) | The regional render latency regression bisected in 13A-5b-iii-c-1 (first slower commit `b7dd168`, largest step `c214a1b` — Measured latency): profile the latency example's flat document, name the per-sample / per-render cost with evidence and remove it, keeping every regional exactness test green; check the typed path too. DEFERRED until the machine is idle (the fix is profiling + interleaved A/B timing against `68650dd`, which concurrent heavy jobs make meaningless); the user runs it when nothing else is running. Must be done (or its residual stated with cause) before 13A closes | the latency example's 512×512 region / divisor-8 preview within 10% of the 13A-1 figures (≈13 ms; ≈18.6 / 37 ms), measured interleaved on an idle machine; output byte-identical to the reference |
| **13A-5c-1** (split from 13A-5c, itself split from 13A-5) | ICC/description preparation (`validate_context`): the byte regional plan charges the description-parse scratch `validate_context` needs (`adjust_calibrated::context_scratch`, the byte cutout's rule, moved), and every description parse the regional render runs — the admission's `validate_context_with_job`, the walk's `from_meta` (a poll per node) and `composite_level_into`'s re-validation inside a materialization — is polled under the render's job with a declared, measured bound; the walk's plan `operator` from `Adjustment::op_name`; the carried reflow nit (`regional_typed_styled_composite_cancel.rs`'s module doc) | declared bound, measured cancel-to-return; full-frame byte-identical; typed path unchanged |
| 13A-5c-2 (split from 13A-5c) | The `inpaint_source.rs` full-frame grouped-source / legacy-promotion sampling fallback (Sample All over groups, styles, metadata, masks needing effective coverage) — tiled/regional with measured cancellation | declared bound, measured cancel-to-return |
| **13A-5c-2a** (split from 13A-5c-2) | The sampling plate's full-frame fallback (`flattened_cut_with_job`) made cancellable under the sampling job (`checkpoints_only`, no progress reported) with a declared, measured bound `T_f + c_row`; the tiled path widened to every mask form, pointwise adjustments and opaque layer metadata (`tile_safe`: a plan-time `pointwise` that parses nothing, a render-time `pointwise_cut`), bound `T_f + c_tile_row`; the tiled reservation `tile_scratch_bytes`, the description-context term `context_scratch` reserved in both arms and covered by `promote_flattened_plan`; the two carried 13A-5c-1 nits | output byte-identical to `rz_doc_flattened` / today's fallback; declared bounds, measured cancel-to-return |
| **13A-5c-2b** (split from 13A-5c-2) | Groups (isolated / pass-through / nested) and raster styles through the tiled loop — a per-tile D5 Background / Shallow snapshot origin rule, styled-group materialization over the full extent, `RenderBudget` semantics per tile — or the byte regional compositor given a `Cut`; decide there. **DECIDED: the byte regional tree given a `Cut`** (`doc_regional_plate::regional_cut_plate`: one admission over the canvas with the cut, one resolve over the canvas clip, ≤ 1024×64 tiles), not the `composite_level_into` tile loop — the tree already takes D5 / Shallow snapshots per sample, materializes over the canvas clip and charges the budget once, as the reference does, and is proven against `rz_doc_flattened` by the `regional_*` suites; its cost, a SAMPLE-MAJOR cancel unit (one tile row of root samples through the whole tree, `c_tree_row`), is declared and measured | output byte-identical to the fallback; declared bound, measured cancel-to-return |
| 13A-5c-3 (split from 13A-5c) | The three "(resource accounting)" open items (the materialization scratch peaks and the uncharged per-unit clip/style/knockout buffers; the D5 Background / Shallow snapshots dry-run and charged; top-level zero-opacity isolated group members in the dry run) and charging the walk's `from_meta` transient on a MODELLED profile (a pin re-base across the byte suites) | declared scratch, pinned; stated differences closed or re-stated |
| 13A-6 | Bounded native-precision selection/artwork/long-stroke coverage processing (no cap raise, no resolution cut) | beyond-cap cases finish at original dimensions |
| **13A-6a** (split from 13A-6) | SELECTION: the typed (u16) Feather / Smooth / Grow / Shrink / Border kernels — and Refine's stages, which ARE those kernels — evaluate over the selection's SUPPORT (the bounding rect of its nonzero samples) grown by the kernel's reach, EXACTLY (`precision_selection_support.rs`; the whole-plane path retained as `reference_apply`); the shared EDT envelope (`doc_select::edt_1d`'s `s` / `z`) in f64 so the cropped and whole-plane morphology agree past coordinate 4096; the work check moves from `plan` to `apply` (same sentence, the window's pixels), Refine keeps its SUMMED check, now support-bounded; the cap survey below; three carried 13A-5c-3 items | a 250-px feather of a small selection on a ≥ 1,996,008-pixel typed canvas (refused before) finishes at the canvas's dimensions, equal to the whole-plane kernel over the window; the cap number unchanged; full-support inputs refuse as before |
| **13A-6b** (split from 13A-6) | ARTWORK: an active-edge sweep in `artwork_coverage` (edges sorted by top y, rows outside the outline's [ymin, ymax] skipped, work `n + Σ active per row`) against the retained full-scan reference; `artwork_render`'s 64M aggregate (one canvas plane per component) decided there. **Implemented 2026-09-24**: `n` charged up front (never `n + E`), the ONE row body `row_coverage` shared with `reference_rasterize`; the 64M aggregate **KEPT** (one EXTENT plane per component; a windowed component contract is 13D's) | beyond-cap outlines finish at the canvas's dimensions, equal to the full scan |
| 13A-6c (split from 13A-6) | **SPLIT into 13A-6c-1 (core) and 13A-6c-2 (Swift)** — LONG STROKES: the brush f64 field over the stroke's FOOTPRINT rect (∪ dab bounds ∩ canvas), exact interior/exterior classification of hard-dab pixels (64/64 and 0/64) with 8×8 integration only on the band, measured per-dab-row cancel; the per-batch vs whole-stroke dab model and the Swift session refusals (`NativeBrushStroke`, `SoftBrush.stampCenters`' silent truncation, the `PrecisionStrokeCoverage` byte adapter) decided there with 13D | a beyond-cap stroke finishes at the canvas's dimensions, equal to the canvas-field reference |
| **13A-6c-1** (split from 13A-6c) | CORE: `brush_coverage::rasterize` accumulates its f64 field over the stroke's FOOTPRINT (∪ nonzero dab boxes ∩ canvas) and classifies hard-dab pixels (diameter ≥ 1) exactly — INTERIOR `1.0·d[3]`, EXTERIOR skipped, the 8×8 sum only on the BAND — against the retained canvas-field `reference_rasterize`; `plan` charges `n + \|F\| + Σ (0 zero / box pixel·soft / min(64·box, box + 64·band_bound) hard)`, never above the old figure; `MAX_BRUSH_DABS`, `MAX_BRUSH_WORK` and both sentences unchanged. **Implemented 2026-09-24**; the WHOLE-STROKE dab model KEPT | a beyond-cap hard stroke finishes at the canvas's dimensions, equal to the canvas-field reference |
| **13A-6c-2** (split from 13A-6c) | SWIFT: `NativeBrushStroke` / `NativeBrushStrokeSession` refusals by name, `SoftBrush.stampCenters`' silent truncation, the `PrecisionStrokeCoverage` ×257 adapter for UNVERSIONED typed MCP point strokes routed through `rz_brush_u16` or refused by name, `QuickMaskStroke`; a cap change is 13D's. **Implemented 2026-09-25**: one `dabLimitRefusal` sentence; the byte walker THROWS it; unversioned 16-bit typed points go through `NativeBrushStrokeSession.generated` and `rz_brush_u16` (the ×257 stroke adapter deleted), now refusing by name at BOTH core limits — a stated contract delta; `QuickMaskStroke` unmodified (it inherits the resolved stroke's refusal) | no silent truncation; typed point strokes at native precision or refused by name |
| **13A-7** | `verify_phase12e2b_placement.py` single-instance collapse (200 worker-backed edits, flat `NSAnimation _runBlocking` count); close 13A in the plan and P7-31 in carryovers. **Implemented 2026-09-25**: the drive runs every gate in ONE owned instance and adds P11 (the parked count read before P1, after P10 and every 20 of 200 more worker-backed `place_image` + `undo` edits, each EXACTLY the starting value, no Dispatch Thread Soft Limit); the per-gate relaunch is removed. The CLOSE of 13A and P7-31 moves to 13A-latency (the plan and the P7-31 row say so); the three carried 13A-6c-2 review nits | drive passes in one instance |

| Run | State |
|---|---|
| 13A-1 | **Implemented 2026-09-23** (this document) |
| 13A-2a | **Implemented 2026-09-23** (this document) |
| 13A-2b | **Implemented 2026-09-23** (this document) |
| 13A-3a | **Implemented 2026-09-23** (this document) |
| 13A-3b-i | **Implemented 2026-09-23** (this document) |
| 13A-3b-ii | **Implemented 2026-09-23** (this document) |
| 13A-3c | **Implemented 2026-09-23** (this document) |
| 13A-4a | **Implemented 2026-09-23** (this document) |
| 13A-4b-i | **Implemented 2026-09-23** (this document) |
| 13A-4b-ii-a | **Implemented 2026-09-23** (this document) |
| 13A-4b-ii-b-1 | **Implemented 2026-09-23** (this document) |
| 13A-4b-ii-b-2 | **Implemented 2026-09-24** (this document) |
| 13A-5a | **Implemented 2026-09-24** (this document) |
| 13A-5b-i | **Implemented 2026-09-24** (this document) |
| 13A-5b-ii-a | **Implemented 2026-09-24** (this document) |
| 13A-5b-ii-b | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-a | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-b-1 | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-b-2 | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-b-3 | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-c-1 | **Implemented 2026-09-24** (this document) |
| 13A-5b-iii-c-2 | **Implemented 2026-09-24** (this document) |
| 13A-5c-1 | **Implemented 2026-09-24** (this document) |
| 13A-5c-2a | **Implemented 2026-09-24** (this document) |
| 13A-5c-2b | **Implemented 2026-09-24** (this document) |
| 13A-5c-3 | **Implemented 2026-09-24** (this document) |
| 13A-6a | **Implemented 2026-09-24** (this document) |
| 13A-6b | **Implemented 2026-09-24** (this document) |
| 13A-6c-1 | **Implemented 2026-09-24** (this document) |
| 13A-6c-2 | **Implemented 2026-09-25** (this document) |
| 13A-7 | **Implemented 2026-09-25** (this document) |
| 13A-latency | **Deferred** — needs an idle machine; resume the litecode run `phase-13a-regional-latency-regression` at its design stage. The LAST 13A sub-slice: 13A and P7-31 close with it |

## 13A-1 contract

The contract's home in code is `core/src/regional.rs`'s module doc; the
compositors are `core/src/doc_regional.rs` (8 bits) and
`core/src/precision_regional.rs` (16 bits and 32-bit float), exposed by
`core/src/ffi_regional.rs`.

**Reference.** The full-frame projection is untouched and is the reference:
`rz_doc_flattened` at 8 bits, `rz_precision_doc_render` at 16 bits and float.
The regional render equals its crop/sampling **sample for sample** — bytes at 8
bits, u16 codes at 16, f32 bit patterns at float. Exactness is the one
tolerance.

**Sample mapping.** Output pixel (i, j) of the window (x, y, w, h) at preview
divisor d is canvas pixel (x + i·d, y + j·d): the top-left pixel of each d×d
cell (the Phase 4 contract's `nearest_top_left_cell`, [phase-4-rendering-contract.md](phase-4-rendering-contract.md)). The output is
ceil(w/d) × ceil(h/d). d = 1 is authoritative; any larger d is a PREVIEW only.

**Admitted set.** A flat stack (every entry top-level) of rasters that carry no
enabled mask, no clip flag and no rendered style (any offset — partly or wholly
off-canvas — any blend mode the depth admits, any opacity), and adjustment
layers whose operator is POINTWISE (`Adjustment::is_pointwise`: every op
except a Shadows/Highlights whose guide is read; zero-amount
Shadows/Highlights is admitted). A DISABLED mask is admitted. Hidden entries are
skipped (at 8 bits zero-opacity entries too, as the reference skips them; the
typed scene keeps a zero-opacity record, which contributes nothing).

**Refusals — sentence and ORDER.** Every other stack is refused by name; there
is no full-frame fallback. The sentence is
`regional render refuses layer {index} ({name}): {construct}`, with `index` the
byte bottom-first stack index or the typed postorder record index. The walk is
bottom-first and checks each entry in this FIXED order:

1. structure — `is a group` (an empty group reached as itself) / `is inside a
   group` (8 bits: a child is reached before its group), **even when hidden**;
2. clipping — `is a clipping-mask member`, **even when hidden or at opacity 0**
   (the reference derives a clip run from `clipped` alone: a hidden member still
   sends its base down `composite_clip_group_into`'s private-buffer path, where
   an adjustment base contributes nothing);
3. the skip — hidden (and, at 8 bits, zero-opacity) entries contribute nothing;
4. pixel constructs — `carries a layer style`, `carries Blend If` (typed only;
   the byte Blend If lives inside the style), `carries an enabled layer mask`;
5. the operator — `is Shadows/Highlights with a guide`.

A construct that changes the reference's PATH refuses whatever its visibility;
one that only changes a visible layer's PIXELS is checked after the skip.
**Plan and render refuse identically:** before the walk the byte plan raises
`adjust_calibrated::validate_context` and `RzDocument::transfer` (the 11E
refusal walk); the typed plan raises `PrecisionDocument::transfer` FIRST (the
11E walk, which is also the float refusal — the same call `scene_for` makes).
Request refusals reuse Phase 4's sentences verbatim ("render region must have
nonzero dimensions", "render region is outside the source image", "render
preview divisor must be at least 1", "render output requires N bytes; budget is
M bytes").

**Plan schema** (`contract_version` 1, both depths):
`{"contract_version":1,"operation":"composite","depth":"rgba8"|"rgba16"|"rgba32f","reference":"rz_doc_flattened"|"rz_precision_doc_render","requested_region":{x,y,width,height},"required_input_region":<same>,"neighborhood":{"kind":"pointwise","halo_pixels":0},"global_analysis":false,"full_frame_fallback":false,"preview_divisor":d,"preview_only":d!=1,"sampling":"nearest_top_left_cell","output":{"width","height","bytes"},"scratch_heap_bytes","work_units","cancellation_checkpoint_pixels":4096,"layers":[{"index","name","kind":"raster"|"adjustment","operator","neighborhood":{...},"global_analysis":false}]}`,
plus `"max_output_bytes"` at 8 bits. (Later deltas: rows gain `clip_base` /
`mask` / `blend_if` in 13A-2a, `group` and the top-level `group_depth` in
13A-3a, and `style` — `null` | `"fill"` | `"effects"` — in 13A-3b-i; since
13A-3b-i the top-level `global_analysis` is `false` or the list of the rows'
distinct `Global` analyses, and with one `required_input_region` is the WHOLE
canvas.) `operator` is `"composite"` for a raster
and the adjustment's op name otherwise. Scratch: 8 bits `ow·oh·16 + ow·oh·4`
(accumulator + output); typed `ow·oh·16 + ow·oh·bpp + BOOKKEEPING_BYTES`.

**Evaluation needs.** One `EvaluationNeed` per contributing layer:
`Pointwise` (13A-1 emits only this), `Spatial { halo_pixels }` (a feathered
mask, since 13A-2a) and `Global { analysis }` — emitted since 13A-3b-i as
`"layer_style_planes"` on a raster whose style renders effect planes (the
planes read the whole layer before the loop); operator analyses are 13A-4's.

**Cancellation and progress.** Single-use `RzJobControl`; a checkpoint every
4096 samples (8 bits `JobContext::every` per sample, typed `Progress::advance`
per 4096-sample chunk); a pre-cancelled control returns "operation cancelled"
with no output and `scratch_bytes() == 0`; a completed render reports stage 5
at 1.0.

**Stated difference from the full render.** A window refuses (a sub-2⁻⁹⁶
effective alpha, a nonfinite kernel output) only when a refusing sample lies
INSIDE it; the full render refuses when one lies anywhere.

## 13A-2a contract deltas

13A-2a extends the 13A-1 contract; everything above not restated here holds.
`core/src/regional.rs`'s module doc is still the contract's home.

**Admitted set.** 13A-1's set plus: layer masks at both depths — raw placement
independent of the layer, density, feather, `outside_coverage` 0/255, enabled
or disabled; CLIP UNITS at both depths — a raster or adjustment base with
raster and adjustment members, hidden and zero-opacity members, hidden bases,
adjustment bases (which contribute nothing over a non-empty run), baseless
clipped entries; and TYPED Blend If on a lone raster, a clip base and a member.

**Units** (both depths, the module doc verbatim): a unit is an unclipped entry
plus the consecutive clipped run above it, or a single baseless clipped entry
below the level's first unclipped entry, which never has members. Consecutive
baseless entries are each their own unit (`doc_group.rs:752-770` composites
each with `&[]` and `i += 1`; `precision_document::render_layers` likewise).
Typed depth uses `scene.layers` as the reference's own builder produced them.

**The hidden-only-run depth difference** (a reference property, mirrored, not
introduced). At 8 bits a unit's `clip_run` is the POSITIONAL run
(`composite_clip_group_into`'s `group.is_empty()`, derived from `clipped`
alone), so a base whose only members are hidden takes the private-buffer path —
and an adjustment base there contributes NOTHING. At 16 bits and float the
scene builder's `clipped_members` holds only the run's VISIBLE records, so the
same run is EMPTY and the base takes the lone arm — an adjustment base there
APPLIES. Each depth's regional render equals its own reference (tests
`an_adjustment_base_whose_only_member_is_hidden_contributes_nothing_at_8_bits`
/ `..._applies_at_u16`, drive gate C3).

**Refusal order** — FIXED, bottom-first over UNITS: (1) structure — `is a
group` / `is inside a group` on every record of the unit, whatever its
visibility; (2) the unit skip — a hidden base (8 bits: or one at opacity 0)
skips its whole run unchecked, a hidden member (8 bits: or one at opacity 0) is
skipped but at 8 bits still makes the run non-empty; (3) `carries a layer
style` on a contributing entry (at 8 bits Blend If and fill opacity live inside
the style, so they refuse here until 13A-2b); (4) `is Shadows/Highlights with a
guide`. REMOVED constructs: "is a clipping-mask member", "carries an enabled
layer mask", "carries Blend If". The sentence is unchanged.

**Per-sample bodies.** No new pixel math. Masks: at 8 bits the per-sample body
of `effective_mask_rect` was HOISTED unchanged into
`Layer::effective_coverage_at` (i64 arithmetic; `effective_mask_rect` now calls
it) and read over `MaskProperties::derived` (now `pub(crate)`), resolved once
per entry before the loop; at typed depth `precision_render::prepare_mask`
(now `pub(crate)`) prepares each unit's masks before that unit's loop, and
`PreparedMask::at` reads them. A clip unit runs `composite_clip_group_into`'s
buffer statements per sample (8 bits: base at opacity 1/Normal, footprint
`a`, members through `composite_layer_sample` / `adjust_backdrop_sample`, alpha
forced to `a`, then `composite_source_into` at `buf[3]·opacity` —
`composite_buffer_into`'s statement); typed: `render_layers`' clip branch per
sample (the `full` clone, `source_weight_16` on the base's own alpha against
the accumulator, `min(a)` clamp under a Blend If base, `checked_alpha(buf[3],
opacity, 1)`). `make test` passed in full after the hoist and the two
visibility changes and before any regional change (2245 passed, 0 failed),
which is the byte-identity check.

**Plan schema deltas** (`contract_version` stays 1). Each `layers[]` row adds
`"clip_base": null|<index>` (the base's index on a member; `null` on every base
and on EVERY baseless entry), `"mask": bool` (an ENABLED mask contributes) and
`"blend_if": bool` (typed; always false at 8 bits). A layer whose ENABLED mask
has feather > 0 declares `{"kind":"spatial","halo_pixels":pad}` with `pad` the
`padded_dimensions` pad (`blur_reach(sigma)`); the top-level `neighborhood` is
spatial with the largest halo when any layer is, and `required_input_region` is
the requested region grown by that halo, clamped to the canvas. Scratch: 8 bits
`ow·oh·16 + ow·oh·4 + Σ mask_properties.scratch_bytes(raw dims)` over
contributing enabled feathered masks (the plane is the mask's padded extent,
never the canvas); typed `ow·oh·16 + ow·oh·bpp + BOOKKEEPING_BYTES + max over
units of Σ mask.plan()?.0`. `work_units` = `ow·oh·(rows + 1)` (typed `+ Σ
mask.plan()?.1`, which `prepare_mask` advances). The plan allocates no pixels
(it never calls `derived` or `prepare`).

**Review nits closed.** (1) The typed walk now admits a DISABLED mask (and an
enabled one). (2) MCP `render`: the "region and preview_divisor apply to the
flattened canvas only" guard runs BEFORE `regionArgument` parses, so a
malformed `region` beside `layer`/`channel` answers the flattened-canvas-only
sentence (drive gate N1). (3) The byte `render_region` uses the transfer
`regional_admission` returns — the 11E walk runs once.

**Decision: `RegionalPlan.canvas`.** The plan carries the canvas dimensions
(`pub(crate)`) beside `halo_pixels`, because `required_input_region` is clamped
to the canvas and the plan JSON is built from the plan alone.

**Cancellation.** Unchanged cadence: 8 bits `JobContext::every` per
unit-sample (a unit-sample evaluates the base and every contributing member at
one output sample), typed `Progress::advance` per 4096-sample chunk. The byte
`derived` feather blur and the typed `prepare_mask` run before a unit's loop;
the byte blur is uncancellable (the reference's own preparation stage, owned
by 13A-5), the typed preparation polls its job. DECLARED BOUND: a cancel
returns within 4096 samples. MEASURED: see "13A-2a verification record".

## 13A-2b contract deltas

13A-2b extends the 13A-2a contract; everything above not restated here holds.
`core/src/regional.rs`'s module doc is still the contract's home. Typed depth is
untouched.

**Admitted set.** 13A-2a's set plus, at 8 bits ONLY, a raster (lone, clip base
or clip member) whose layer style has NO enabled effect and NO knockout — fill
opacity, Blend If, disabled effects, `blend_interior_as_group`. The reference
for such a style is `composite_styled_into` with `planes = None`, so always
`composite_direct_into`; `routes_unit` is false (no planes, `Knockout::None`)
and `members_want_base_snapshot` is false once knockout members refuse. So Blend
If is admitted at every depth and fill opacity at 8 bits only; every 16-bit or
float layer style still refuses (`precision_regional.rs`, 13A-3).

**Refusal order** — unchanged in shape: (1) structure, (2) the unit skip, (3)
pixel constructs, (4) the operator. Step (3) at 8 bits is now, per contributing
entry with a rendered style: `carries a layer style` (some `Effect::enabled`),
THEN `carries a knockout` (`style.knockout != Knockout::None`); a style with
neither is admitted. Plan and render refuse identically; every 13A-1/2a
sentence and assertion stays valid.

**Per-sample bodies.** No new pixel math. `composite_pixels_into`'s per-pixel
body was EXTRACTED statement for statement into
`style_composite::composite_pixels_sample` (with the loop-invariant
`PixelPass`), and `Shape::of_layer`'s per-pixel coverage into
`Shape::coverage_byte`; the full-frame loops call them. A lone styled raster
samples `composite_pixels_sample` with `under = acc[ai]` (the reference has no
snapshot there and reads the accumulator itself); a styled clip base samples it
into the unit buffer at opacity 1/Normal with `under = acc[ai]` as it stood
before the unit (the reference's full-rect `BackdropSnapshot`); a styled member
reads the unit buffer itself. After each visible member a STYLED base clamps
the buffer alpha to `Shape::coverage_byte(alpha, effective mask)/255` (0 outside
the layer rect) — `clamp_alpha_to_shape` over `Shape::of_layer(layer, 0)` —
unless the layer exceeds `MAX_PIXELS`, where the reference falls back to the
pass's own alpha (`base_alpha`), exactly as an unstyled base does. `make test`
passed in full after the two extractions and before any regional change (2274
passed, 0 failed, 241 binaries), which is the byte-identity check.

**Decision: the clamp-only step.** The reference's member loop is
`group.iter().filter(|l| l.visible)` (`doc.rs:585`, `style_composite.rs:423`) and
`composite_layer_into` returns at once for a member at opacity 0 — but the
post-member alpha statement after it STILL runs. Under a styled base that is a
real clamp (the fixture `a_visible_zero_opacity_member_still_clamps_a_styled_base`:
alpha 143 × mask 115 → pass alpha 0.2529 clamped to 64/255, 51 with the clamp,
52 without). So a byte unit's `members` became `steps`: hidden members are
DROPPED, a visible zero-opacity member is a `Step::ClampOnly` — never inspected
by (3)/(4), no plan row, no work unit — and a contributing one a
`Step::Member`. Under an unstyled base the clamp-only step is the `base_alpha`
reset, a no-op.

**Plan schema.** `contract_version` stays 1; no new field. `layers[].blend_if`
is now true at 8 bits on a contributing raster whose style carries a
non-identity Blend If. Scratch and work formulas are unchanged.

**11E.** A styled layer is refused by the 11E `linear_light` walk, so the
`transfer` is `None` whenever a style is admitted and `composite_pixels_sample`
(which decodes nothing, as `composite_pixels_into` never did) is exact; that is
why there is no linear-light styled case (tested: `with_evaluation(LinearLight)`
on a styled document is `Err`).

**Cancellation.** No new stage: the cadence and the declared 4096-sample bound
are unchanged.

**Review nits closed (from 13A-2a).** (1)
`the_typed_plan_declares_masks_halos_and_formulas` asserts the exact formulas:
`work_units == pixels·5 + plane·32 + plane·2·taps` and `scratch_heap_bytes ==
unmasked + plane·32 + taps·4 + 4096`, with `plane` the padded mask extent and
13 taps (feather 6 → factor 1 → radius 6), `plane_blur::float_plan` restated.
(2) `a_concurrently_cancelled_masked_clip_render_returns_operation_cancelled_at_u16`,
the u16 twin of the byte concurrent-cancel test.

## 13A-3a contract deltas

13A-3a extends the 13A-2b contract; everything above not restated here holds.
`core/src/regional.rs`'s module doc is still the contract's home.

**Admitted set.** 13A-2b's set, now over a TREE of levels: the root stack and
every group's children, nested to the depth cap. GROUPS are admitted at 8, 16
and float — isolated (a blend mode other than Pass Through, a style, a clip
base with a VISIBLE member, a clipped group with a base, and every group MEMBER)
and pass-through (plain, or gated by a sub-1 opacity and/or an enabled mask) —
as lone entries, clip bases, clip members and baseless clipped entries, hidden
(the subtree is skipped unchecked) or empty. At 8 bits an effect-less style
(fill opacity, Blend If) is admitted on a group as on a raster; typed Blend If
on an isolated group is admitted. Every typed style and every 8-bit style with
an enabled effect still refuses "carries a layer style" (13A-3b); a knockout
still refuses (13A-3c).

**Refusal order** — FIXED, bottom-first over the units of a level: (1) the
unit skip (a hidden base, 8 bits: or one at opacity 0, skips its run AND every
subtree in it unchecked; a hidden member is dropped, a visible zero-opacity
member is a clamp-only step); (2) on every contributing entry of the unit, in
order, "carries a layer style" then "carries a knockout" (a group's style is
read through `Layer::style` — a group's meta is never interpreted — its
isolation through `doc_group::is_isolated`, which reads `renders_style`); (3)
on every contributing entry, in order: a group's children level is walked in
this same order and the group's row pushed AFTER theirs, an adjustment is
checked for "is Shadows/Highlights with a guide". The "is a group" / "is
inside a group" refusals are REMOVED; the sentence is unchanged. A byte group
the reference refuses for its nesting (a contributing group entry at depth ≥
`doc_group::MAX_GROUP_DEPTH`, which `rendered_group` / `composite_level_into`
refuse) refuses regionally in the reference's own words, "rendering exceeds
the group nesting limit" (test
`a_group_past_the_reference_nesting_limit_refuses_in_its_words`).

**Byte path choice** (the accepted objection). The byte sampler branches on
the base's CONTRIBUTION first, mirroring `composite_unit_into`, which tests
`!is_isolated` before anything else: a `PassThrough` base ALWAYS composites
its children straight onto the sample (opacity 1, no enabled mask) or onto a
copy lerped back by `gate_source_into` at `coverage · opacity`
(`composite_gated_pass`), whatever its positional `clip_run` — its `steps`
are empty by construction (a visible member would have isolated it).
`clip_run` is inspected only for a raster, adjustment or isolated base,
exactly as 13A-2b's loop. At typed depth the unit test is the reference's own
`clipped_members.is_empty()` (`render_layers`), so the same hazard cannot
arise. Pinned by the objection's fixture at both depths — an opaque backdrop,
a pass-through group P holding an Invert, a HIDDEN raster clipped to P: at P
opacity 1 the backdrop is inverted exactly (255 − v / 65535 − v), at 0.5 it
is half inverted (127–128 / 32767–32768) — and by drive gate G3. A
`clip_run`-first mutation of the byte sampler fails that test (checked).

**Isolated byte groups.** An isolated group's sample is its children's level
evaluated on a transparent sample, encoded at the boundary by
`doc_evaluation::encode_sample` (the extracted body of `encode_projection`,
`doc::quantize_sample` on the encoded path) and composited as
`rendered_group`'s synthetic RASTER: `composite_layer_sample` (or
`composite_pixels_sample` under an effect-less style, `Shape::coverage_byte`
for a styled base's clamp) with the group's opacity, its mode (Pass Through →
Normal, `is_group_only`) and coverage `Layer::effective_coverage_at` over its
own derived mask — the synthetic's cropped `effective_mask_rect`, on which
`aligned_mask` is the identity. The synthetic covers the group's EXTENT
(`doc_group::group_extent`, now `pub(crate)`, intersected with the enclosing
rect exactly as `rendered_group` intersects it); outside it the target is
untouched. **Decision: an empty extent** (nothing visible inside, or nothing
inside the canvas) renders to NOTHING, as `rendered_group`'s `None`: as a
base the whole unit contributes nothing (`composite_unit_into`'s `None =>
return`), as a MEMBER it is dropped from the member loop — `resolved_member`
keeps it only as an INVISIBLE placeholder — so no post-member clamp runs for
it. The difference is observable under a styled base (the 13A-2b 51/52
fixture: an empty or off-canvas group member leaves 52, a member group with an
extent elsewhere on the canvas clamps to 51; test
`an_empty_extent_group_member_is_dropped_under_a_styled_base`).

**Typed groups.** `precision_regional_tree::prepare` builds the node tree from
`scene.layers` once — every mask through `prepare_mask` (a node whose
whole-layer Adjustment skip holds is not prepared, its work advanced
unprepared, as the reference never prepares it), each clip base's `full` clone,
children of `Kind::Group | Kind::PassThrough`, members exactly
`clipped_members` — and `sample_layers` is `render_layers` per sample:
`Kind::Group` samples its children onto a transparent sample and runs
`composite_group_sample16`; `Kind::PassThrough` samples them onto a copy of
the sample and runs `gate_pass_through_sample16`. A float document's
`group_layers` mints the container ISOLATED (the float admitted set has no Pass
Through), so the float fixture's group rows are "isolated".

**Per-sample bodies.** No new pixel math. EXTRACTED statement for statement:
`doc::quantize_sample` (from `quantize`), `doc_evaluation::encode_sample`
(from `encode_projection`), `precision_render::composite_group_sample16` (the
`Kind::Group` arm's closing loop: `checked_alpha` → Blend If weight against
`acc[i]` → `composite_source_into`, no transfer decode — the group buffer
already holds composited components) and
`precision_render::gate_pass_through_sample16` (the `Kind::PassThrough` arm's
floor check then `gate_source_into`); the full-frame loops call them. `make
test` passed in full after the extractions and before any regional change
(2285 passed, 0 failed, 242 binaries), which is the byte-identity check.

**Sampling.** The window is walked SAMPLE-MAJOR at both depths: each ROOT
sample evaluates the whole tree (every row) before the next — one pass over
the window, `job.every` per root sample at 8 bits, `progress.advance((end −
start) · rows)` per 4096 root samples at typed depth. Nit 1 closed: the byte
walk builds `Step`s directly (no `order` vector).

**Plan schema deltas** (`contract_version` stays 1). Every `layers[]` row
gains `"group": null|<index>` (the enclosing group's byte bottom-first /
typed postorder index); `kind` gains `"group"`, whose `operator` is
`"isolated"` or `"pass_through"` (the reference's decision for that entry); the
top level gains `"group_depth"` (the deepest contributing group nesting: 0
with no group, 1 for a root-level group). Rows are in evaluation order, which
is ascending index at both depths (a group's row follows its children's).
Scratch: 8 bits `ow·oh·16 + ow·oh·4 + Σ mask scratch` over every contributing
enabled feathered mask, groups included; typed `ow·oh·16 + ow·oh·bpp +
BOOKKEEPING_BYTES + Σ mask.plan()?.0` over EVERY contributing mask — all are
prepared before the loop and held together (was the max over units; the
existing single-mask assertions are unchanged in value). `work_units =
ow·oh·(rows + 1)` (typed `+ Σ mask.plan()?.1`), rows counting group rows. A
feathered group mask declares its `Spatial` halo on the group's row;
`required_input_region` is as before.

**Stated differences** (module doc and here). No per-level buffer is
allocated, so `MAX_GROUP_BUFFER_PIXELS` / `RenderBudget` refusals have no
regional counterpart: a nest the reference refuses for its budget, or a
zero-opacity isolated base whose extent (and nesting limit) the reference
still charges, renders regionally. Floor/nonfinite refusals only inside the
window, as before.

**11E.** `refused_blend` admits Normal and Pass Through (and Multiply and the
other separable modes), so a `linear_light` document with groups is
admissible; at an isolated byte boundary `encode_sample(.., Some(transfer))`
then `composite_layer_sample`'s decode — `rendered_group`'s order. Tested at 8
and 16 bits with a nested Multiply group.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096 ROOT
samples (each evaluating every row at that sample). MEASURED: see the 13A-3a
verification record.

**Review nits closed (from 13A-2b).** (1) `doc_regional.rs`'s `order` vector
is gone: steps are built directly while walking the run. (2)
`verify_phase13a2b_byte_styles.py` gate S3 asserts the `zero` row is visible at
`opacity == 0` and the `hidden` row `visible is False` before the first
`regions_equal_crops`.

## 13A-3b-i contract deltas

13A-3b-i extends the 13A-3a contract; everything above not restated here holds.
`core/src/regional.rs`'s module doc is still the contract's home. **13A-3b was
SPLIT** (plan table above): this run is 13A-3b-i; styled GROUPS with enabled
effects are 13A-3b-ii.

**Admitted set.** 13A-3a's set plus EVERY layer style on a RASTER at 8 and 16
bits — enabled effects (every effect: drop shadow, outer glow, the three
strokes, colour and gradient overlays, inner shadow, inner glow, bevel, satin),
fill opacity, Blend If, disabled effects, the package model at opacity below 1
— as a lone entry, a clip base, a clip member and inside isolated and
pass-through groups; and at 16 bits an effect-less (fill / Blend If) style on an
isolated group, lone, as a clip base and as a member (the byte depth admitted
that in 13A-3a). A float document carries no style: the float policy refuses a
style before the walk (`float_document.rs:464`), and `RzDocument::transfer` /
`PrecisionDocument::transfer` refuse one under linear light, so `transfer` is
`None` whenever a style is admitted.

**Refusal order** — (1) the unit skip; (2) on every contributing entry of the
unit, in order, entry by entry: "is a group with layer-style effects" (a GROUP
whose style `has_enabled_effects()` — 13A-3b-ii), "carries a knockout"
(`knockout != None` — 13A-3c), "blends interior effects as a group"
(`blend_interior_as_group && style_knockout::has_grouped_interior_effect`,
whatever the members — 13A-3c; on a clip base with visible members the
reference would not route, so this over-refuses there, stated and owned by
13A-3c); (3) as before. "carries a layer style" is REMOVED at both depths. The
sentence is unchanged; plan and render refuse identically.

**Per-sample bodies.** No new pixel math. EXTRACTED statement for statement:
`style_composite::composite_contribution_sample` (the loop body of
`composite_contribution`), `precision_style_render::composite_pixels_sample16`
with the loop-invariant `PixelPass16` (the typed `composite_pixels_into`'s
body) and `typed_coverage_byte` (`typed_shape`'s coverage statement);
`StyledPixels::alpha16` became `pub(crate)`. The full-frame loops call them.
`make test` passed in full after the extractions and before any regional
change (2314 passed, 0 failed, 244 binaries), which is the byte-identity check.
A styled sample runs `composite_styled_into`'s order at one sample,
target-agnostic (the accumulator for a lone node, the unit buffer for a
member): `under` read before anything (the Blend If snapshot's value
everywhere the pass reads it — nothing Below lands outside `below_landing`);
each Below contribution whose plane pixel lands on the sample
(`doc_regional_tree::contributions_sample`, index arithmetic around
`composite_contribution_sample`, shared by both trees); the pixel pass —
lone: onto the target with `under`; clip base: into a private sample at
Normal/1, each step, the alpha clamped after each step to the planes' shape
(`Shape::at`) or, without planes, the pad-0 shape (`Shape::coverage_byte` at
8 bits, `typed_coverage_byte` over `alpha16` at 16 — a group's is `g[3] ·
65535`, the `Buffer` arm), 0 outside the layer rect, then
`composite_source_into` at `alpha · opacity` (`composite_buffer_into`'s
statement, no `checked_alpha`); each Interior contribution. The PACKAGE
(planes, opacity below 1, not Dissolve): outside `affected_window` ∩ the
enclosing accumulator the sample is untouched; inside, the steps run at
opacity 1 on a copy and `mix_premultiplied` mixes it back — ALWAYS, even where
nothing landed, because the mix is not an identity in f32. Planes are resolved
once before the loop: byte `LayerStyle::rendered` (the reference's cache) with
`RenderContext { env.light, env.canvas, layer.offset }`; typed `typed_shape`
over the prepared mask + `render_effects`, polling the job before and after
and advancing the style work; each contribution's colour converted once
(`c.color.converted(t)`). A byte styled raster whose padded plane exceeds
`MAX_PIXELS` renders no planes (`rendered` is `None`), exactly as the
reference, and samples as an effect-less style. A styled typed node has no
`full` clone and no whole-layer skip (`render_layers` takes `render_styled`
before the clip branch).

**Plan schema deltas** (`contract_version` stays 1). Every `layers[]` row
gains `"style"`: `null` (no rendered style), `"fill"` (no planes: fill
opacity / Blend If, or at 8 bits a plane over `MAX_PIXELS`) or `"effects"`.
An `"effects"` row declares `Global { analysis: "layer_style_planes" }` —
`{"kind":"global","halo_pixels":null}` with `"global_analysis":
"layer_style_planes"` — and contributes NO halo (its mask's feather is
subsumed); the top-level `global_analysis` becomes the list of distinct
analyses (`["layer_style_planes"]`) and `required_input_region` the WHOLE
canvas. A plan without an `"effects"` row emits exactly what it emitted
before (the top-level `neighborhood` still carries the largest halo). Scratch:
`+ Σ style_scratch(style, dims, false, None).0` over the `"effects"` rows at
both depths; typed `work_units + Σ style_scratch(..).1` (what `prepare`
advances); byte `work_units` unchanged. The plan allocates no pixels
(`plane_pixels` / `style_scratch` only).

**Decision: `Global`, not `Spatial`** (the accepted objection). Plane
rendering reads the whole layer before the loop — `Shape::of_layer`'s bbox
(a layer-aligned gradient spans it), interior `distance` offsets (inner
shadow, satin) — so no halo of `pad + shadow shift` contains the real input.
Pinned by the reach tests at both depths: erasing ONE pixel at the layer's
far corner, outside the window, changes the window's samples for an aligned
gradient overlay and an inner shadow at distance 40, the plan declares the
whole canvas, and the regional render still equals the crop. A tighter,
layer-rect-bounded `required_input_region` is 13B's decision.

**Decision: the float assertion.** No styled record can be constructed in a
float document through the public surface: the `layer_style` edit refuses
with the float policy's sentence, and `PrecisionDocument::build`'s storage
gate admits no u16 leaf beside F32 ones. The test asserts that edit refusal
and, for a record the float policy refuses (curves), that the plan, the
regional render and the full render refuse with one sentence.

**Cancellation.** Byte `rendered` and typed `render_effects` are the
reference's own uncancellable preparation stages (recorded beside the byte
mask blur, 13A-5). DECLARED before measuring: a cancel returns within 4096
ROOT samples once the loop has begun. MEASURED: see the 13A-3b-i verification
record.

**Review nits closed (from 13A-3a).** (1) The header and `ffi_regional.rs` no
longer say "There is no structure refusal": the reference's nesting-limit
refusal is named. (2) The `MAX_GROUP_BUFFER_PIXELS` / `RenderBudget` open item
is re-owned to 13A-3b-ii (the run that materializes group buffers). (3)
`precision_regional.rs`'s mask comment now says a whole-layer-skipped
adjustment's mask is charged but never prepared.

## 13A-3b-ii contract deltas

13A-3b-ii extends the 13A-3b-i contract; everything above not restated here
holds. `core/src/regional.rs`'s module doc is still the contract's home.

**Admitted set.** 13A-3b-i's set plus every layer style on an isolated GROUP
at 8 and 16 bits — a group whose style has an enabled effect (every effect,
the package model at opacity below 1, fill + Blend If beside effects, the
group's own mask at any feather) — lone, as a clip base (members clamped to
its planes' shape, clamp-only steps included), as a clip member under an
unstyled and a styled base, inside plain / gated pass-through and isolated
groups, nested (styled inside styled), over any admitted children
(adjustments, masked rasters, clip units, gated pass-through and isolated
subgroups, hidden and zero-opacity entries). Float carries no style (the 3b-i
assertion pattern holds). Under 11E `transfer` is `None` whenever a style is
admitted, and `with_evaluation(LinearLight)` on a styled-group document is
`Err` at both depths (asserted).

**Refusal order** — (1) the unit skip; (2) on every contributing entry of the
unit, entry by entry: "carries a knockout", then "blends interior effects as
a group" (both 13A-3c); (3) as before. "is a group with layer-style effects"
is DELETED at both depths. A group's knockout beside its effects is named by
the GROUP before any child's (its unit pass precedes its children's walk); a
knockout or a guided Shadows/Highlights inside a materialized subtree is
named by its own index (the children are still walked); a hidden styled
group is skipped unchecked.

**The materialization rule.** 8 bits: an isolated group (the walk's
`doc_group::is_isolated`) whose `Layer::style` has an enabled effect is
`Contribution::Materialized`; the render resolves it through
`doc_group::rendered_group` ITSELF — the synthetic raster over the group's
extent within the enclosing rect, charged against the render's
`RenderBudget` — and samples it as a STYLED RASTER: its own cropped mask
(`MaskProperties::at(origin)`, derived), its style through `Styled::of`
(planes by `LayerStyle::rendered` over the synthetic, the window within the
enclosing rect), its blend (Normal for a group-only mode). Every pixel,
mask, offset and opacity read goes through `ResolvedNode::layer()`. An empty
extent drops the unit or the step exactly as an empty isolated group's. Note
that `rendered_group`'s rect is the CHILDREN's union within the clip: a
group's own style grows only its PARENT's extent (`extent_of_level`), its
effects spill through the padded planes. Typed: a `Kind::Group` whose style
has an enabled effect is prepared as `render_styled` prepares it — the
children rendered by `precision_render::render_layers` into a canvas-sized
buffer (`KnockoutBackdrop::NONE`), `typed_shape` over `StyledPixels::Buffer`
and the prepared mask, `render_effects` with `layer_offset` (0, 0), the
window from `affected_window((0, 0), …)` — and the pixel pass reads the
buffer (`StyledPixels::Buffer::alpha16`); its children are not prepared. An
effect-less styled group is unchanged at both depths. No new pixel math.

**Plan.** `contract_version` stays 1, no new field. At 8 bits every
`Contribution::Materialized` row with a non-empty rect is `Global {
analysis: "group_materialization" }` (`MATERIALIZATION_ANALYSIS`, beside
`STYLE_PLANES_ANALYSIS`) and contributes no halo: `rendered_group` reads the
whole extent before the loop, so the input is the whole canvas EVEN WHEN the
synthetic renders no planes — its `"style"` is `"effects"` iff the padded
plane of the synthetic's rect is within `MAX_PIXELS`, else `"fill"` (pinned
by a 30000 × 1073 canvas: Global, whole-canvas input, no style term). An
empty-rect materialized row is `"fill"` with the ordinary need. At typed depth
every `Content::Group` row whose style `has_enabled_effects()` is `"effects"`
+ `Global {"group_materialization"}`. `global_analysis` lists the distinct
analyses in row order (a styled raster below a styled group:
`["layer_style_planes", "group_materialization"]`). Children rows of a
materialized group are ordinary rows (`group` = the group's index, needs and
halos as today).

**Scratch and work.** 8 bits, after the walk, per materialized group the
reference renders: `peak·16 + |rect|·4 + (enabled mask)·|rect| +
(renders_planes ? style_scratch(style, rect dims, false, None).0 : 0)`, where
`peak` is the dry run's (below) — DECLARED as "the budgeted group buffers
plus the synthetic, its mask crop and its planes"; the reference's
uncharged per-unit buffers inside a materialization (clip and style buffers)
are not modelled (out of scope). `work_units` unchanged (preparation is not
counted, the byte precedent). Typed, per materialization root:
`canvas·16·(1 + depth) + canvas·4·clip_levels + Σ style scratch +
style_scratch(style, canvas, false, None).0` scratch and `canvas + Σ work +
style_scratch(…).1` work, where the `Subtree` mirrors `PrecisionScene::plan`'s
`collect` over the materialized children: `depth` the deepest nesting of
buffer levels (a contributing isolated or pass-through group and a clip unit
with a visible member each one level — the `render_layers` `zero_plane`s and
the pass-through copy), `clip_levels` the deepest count of live clip levels
(each a base-alpha record), style scratch per styled row with `collect`'s own
`blend_if` / `package` arguments, work per row as `collect` counts it (raster
intersection, adjustment canvas + guide work, group 2·canvas, clip unit
3·canvas, styled + style work). A nested materialized group inside the
subtree is one more level plus its style term. SUMS, not the reference's
peaks: conservative. A styled raster inside a typed materialized subtree is
charged twice (its ordinary row as in 13A-3b-i, and the subtree's sum) —
conservative, recorded. `MAX_WORK_UNITS` is NOT applied to the typed regional
`work_units` (the reference's `collect` refuses past it; the regional figure
drives progress only) — stated, out of scope.

**The budget counterpart.** The 8-bit plan collects, in walk order, every
materialized group the reference renders — never one inside another
materialized subtree (the dry run recurses there), never a member of a unit
whose base is a group with an empty extent (`composite_unit_into` returns
before `resolved_member`) — and AFTER the walk (structure refusals first)
dry-runs each against `RenderBudget::new(group_pixels)` with
`doc_regional_style::materialization_charges`: statement by statement the
charge and refusal points of `rendered_group` (the nesting limit, the empty
extent, the buffer charge held while the children run), `composite_level_into`
(its depth check, the `visible` guard), `composite_unit_into` (a pass-through
group's Shallow snapshot and gated copy, an isolated base's recursion whose
`None` skips the members) and `resolved_member` (sequential), guarded by
`visible`, allocating nothing. The render resolves with a fresh budget of the
same size, charging exactly as the dry run did. Plan and render therefore
refuse with the reference's own sentences — "rendering exceeds the group
buffer budget (N pixels); …" and "rendering exceeds the group nesting limit" —
and never refuse a render the reference completes. `render_region` /
`render_region_plan` keep their signatures and call the `#[doc(hidden)]`
`render_region_with_group_budget` / `render_region_plan_with_group_budget`
twins with `MAX_GROUP_BUFFER_PIXELS` (the `flattened_with_group_budget`
pattern; `RegionRequest` is re-exported `#[doc(hidden)]` from `lib.rs` so
black-box tests can reach them). STATED DIFFERENCE: only materialized nests
are charged, so a reference budget refusal caused by an unmaterialized level
(an isolated or gated group sampled per sample, a zero-opacity isolated base
the reference still charges, D5's root snapshot) does not refuse regionally —
pinned: an unstyled Multiply outer group 30×20 over a 300-pixel styled group
refuses the reference at 899 and admits and equals it regionally.

**Cancellation.** Byte `rendered_group` and `LayerStyle::rendered` are the
reference's own uncancellable preparation (owned by 13A-5, beside the mask
blur); typed `render_layers` polls per layer and per 4096-pixel chunk.
DECLARED before measuring: a cancel returns within 4096 ROOT samples once the
loop has begun. MEASURED: see the 13A-3b-ii verification record.

**Review nits closed (from 13A-3b-i).** (1) `doc_regional_tree.rs` is split:
`doc_regional_style.rs` holds `Styled` (+ `Styled::of`, extracted from the
raster arm), `contributions_sample` and the styled per-sample bodies moved
verbatim (`styled_sample`, `direct_sample`, `pixel_pass`, `shape_byte`),
plus `resolve_materialized` and `materialization_charges`; the tree is 543
lines. (2) `regional.rs`'s 128-character module-doc line is rewrapped. (3)
`precision_regional_tree.rs`'s 158-character match arm reads the
`UNSTYLABLE` const. `make test` passed in full after the three nits and
before any behaviour change (2342 passed, 0 failed) — the byte-identity
check.

## 13A-3c contract deltas

13A-3c extends the 13A-3b-ii contract; everything above not restated here
holds. `core/src/regional.rs`'s module doc is still the contract's home.

**Admitted set.** 13A-3b-ii's set plus KNOCKOUT (None / Shallow / Deep) and
"Blend Interior Effects as Group" (D6) on every style the regional path
already admits, at 8 and 16 bits: a lone raster over D5's Background (and
over a backdrop that fails any clause of `background_entry` — offset,
opacity, an enabled mask, entry 1 clipped, hidden, a non-Normal mode, a
style, typed Blend If — where the hole is transparent), beside Blend If,
under Dissolve, masked (feather 0 and 6), partly off canvas, in the package
(opacity 0.6 Normal / Multiply with effects); a Shallow / Deep child inside
plain, gated and masked pass-through groups and nested pass-through groups;
inside isolated and materialized groups (the hard stop); a knockout MEMBER
under an unstyled, styled (fill, Blend If, effects, package), masked and
adjustment base, after a member that altered the buffer, as a group member
(effect-less and materialized); a knockout clip base with visible and
clamp-only members; D6 on a lone raster, on a base WITH visible members (it
evaluates OFF — the former over-refusal is gone), with hidden-only members
(ON), on a member (ON), on a materialized group, beside a knockout. Float
carries no style (asserted: the style edit refuses a knockout and a grouped
interior on the float fixture). Under 11E a styled layer is refused, so
`transfer` is `None` wherever a knockout is admitted
(`with_evaluation(LinearLight)` is `Err` at both depths, asserted).

**Refusal order** — (1) the unit skip; (2) entry by entry, a group's own
level (recursively, its row after its children's) or "is Shadows/Highlights
with a guide". Step (2) of 13A-3b-ii — "carries a knockout", then "blends
interior effects as a group" — is DELETED at both depths, with its imports
(`Knockout` / `has_grouped_interior_effect` in the byte walk). The tests
whose point was ORDER now name a guided Shadows/Highlights member through an
admitted knockout (`regional_byte_effects`, `regional_precision_styles`,
`regional_groups`, `regional_byte_styles`, `regional_masks_clips`,
`regional_precision_masks_clips`); the rest flip to admission-equals-reference.

**The unit pass, per sample** (`core/src/regional_knockout.rs`, NEW; no
admission logic, no pixel math). A style the reference routes
(`style_knockout::routes_unit` over its planes and D6's structural flag) runs
`composite_interior_unit_into` at ONE sample: return outside the (synthetic)
layer's rect (the mix loop covers the rect only) and where `s = alpha·mask`
is 0; the pixel pass into a transparent unit sample through
`composite_pixels_sample` / `composite_pixels_sample16` at `fill_in` (1 under
D6), Normal, opacity 1, `direct`, Blend If reading `under` (the full-rect
snapshot's value — the sample before the node); then D6's grouped
contributions (`grouped_interior_sample`: each `is_grouped_interior`
contribution through `contributions_sample` at scale 1, the alpha restored
after each) OR a clip base's members (steps non-empty ⇔ the reference's
`group.iter().any(visible)`), each composited onto the unit with the
base-only sample as BOTH backdrops when some member carries a knockout
(`members_want_base_snapshot` / `members_want_base_snapshot16`), the alpha
clamped to `footprint_limit` (the planes' shape, else the pad-0 shape when
`shaped`, else reset); then `q = fill_out·min(1, U.a/s)`, `K =
backdrop_at(knockout, style.knockout, target, 0)` over-composited by `U` at
`q` with the layer's mode, and `target = mix_premultiplied(target, K,
s·opacity)`. Below, the non-grouped Interior contributions and the package
are unchanged (the package runs the unit on its copy, and the strip-windowed
backdrop IS the same sample). Byte dispatch: `ResolvedUnit::sample` and a
member's `composite` take `styled_sample` when `planes.is_some() ||
routed()`; `direct_sample` accepts `planes: None` when routed. Typed:
`styled_sample16` is already taken for every styled node; `direct_sample16`
branches on `style.routed` first.

**Where each backdrop is born** (the reference's own
`style_knockout::KnockoutBackdrop` over one-element slices; no new type).
ROOT, D5: 8 bits `Level.background` = `composite_projection_into`'s test
verbatim (`document_wants_background && background_entry(.., Cut::NONE) &&
top_level(..).first()` is entry 0 alone), set by `regional_admission` on the
root level only; typed `scene.knockout_seed == Projection { background: true
}` (`PrecisionScene.knockout_seed` made `pub(crate)` — the ONLY
reference-side edit). Then per sample the first unit / node with `NONE`, the
sample right after it snapshotted, every later one with it as both planes.
PASS-THROUGH (plain and gated, both depths): the children get `{shallow:
the sample as they began, deep: the enclosing deep}` ALWAYS — the typed arm's
rule; the byte reference snapshots only when `level_wants_shallow`, and only
a visible Shallow entry of that level reads the plane, so the two agree
sample for sample (pinned by a pass-through group with no Shallow child under
a level that has one). The byte tree carries no `wants_shallow` flag;
`level_wants_shallow` stays in `unit_charges` only. ISOLATED and MATERIALIZED
groups: `NONE` (the pixel's children sample, the typed `Kind::Group` arm and
the non-materialized styled group's children; a materialized group is the
reference's own `rendered_group` / `render_layers`). CLIP MEMBERS, all three
member loops (the unstyled clip buffer — typed after the base's Blend If
weighting, as the reference clones it — the styled base's loop and the unit
pass's): the unit sample right after the base's pixel pass, as both planes,
when some member carries a knockout, else `NONE`. D6's "visible members" is
the base's `Node.visible_members` (the DOCUMENT's positional-run flag, false
on a member — `member_env` resets it) at 8 bits and
`PrecisionLayer::visible_members` at 16.

**Tree changes.** Byte: `Node { visible_members }`, `Level { background }`
(and `ResolvedLevel`), `shaped` moved from `ResolvedUnit` to `ResolvedNode`
(computed in `Node::resolve` / `resolve_materialized` exactly as before),
`KnockoutBackdrop` threaded through `ResolvedLevel::sample`,
`ResolvedUnit::sample`, `ResolvedNode::composite`, `styled_sample` and
`direct_sample`; `Styled { knockout, routed, grouped }` from `Styled::of(..,
visible_members)`; `ResolvedNode::routed` / `footprint_limit` /
`ResolvedStep::wants_base`. Typed: `PreparedStyle` fields `pub(crate)` plus
`knockout`, `routed`, `grouped` (from `prepare_style`); backdrops ride
`StyleEnv.knockout`.

**Plan.** `contract_version` 1, no new field or row field: a knockout reads
the already-evaluated backdrop at the SAME sample — pointwise. A knockout-only
or D6 style row is `"fill"` / `"effects"` as before. 8 bits: no new scratch
(the regional path allocates nothing for a knockout). Typed: inside a
MATERIALIZED subtree each row adds `collect`'s D8 term
`knockout_scratch(routed, grouped, knockout_member, canvas)` to the subtree's
style scratch, with `routed` / `grouped` as `collect` computes them from the
row's `visible_members` (`j == i && rows.len() > 1`) and `knockout_member`
(`j == i` and a visible member carries a knockout) — restated exactly by
`regional_precision_knockout` (a Deep child +16·canvas, a grouped child
+20·canvas, a grouped clip base with a visible member +0, a Shallow member
+32·canvas); sums, conservative.

**Budget — STATED DIFFERENCE, pinned.** The reference holds two knockout
snapshots across later units that the regional path never allocates: D5's
Background snapshot (`composite_projection_into`, canvas pixels across the
root level after entry 0) and an UNMATERIALIZED pass-through level's Shallow
snapshot (`composite_unit_into`, gated by `level_wants_shallow`) — inside a
materialized nest `unit_charges` already pays the latter (pinned: a visible
opacity-1 Shallow child in a materialized nest's pass-through subgroup
refuses at 599 and equals at 600, at plan, render and reference). Neither is
dry-run or charged regionally, so a reference budget refusal caused by either
does not refuse regionally — pinned on a 100×100 canvas (entry 0 an opaque
Background, entry 1 a Deep raster, entry 2 an isolated group with a drop
shadow over a 100×60 child): `flattened_with_group_budget(12000)` refuses
with the budget sentence while `render_region_with_group_budget(.., 12000,
..)` renders every window equal to the reference taken at
`MAX_GROUP_BUFFER_PIXELS`; the same with entry 1 a plain pass-through group
holding a visible Shallow child. Recorded in `regional.rs`'s STATED
DIFFERENCES, beside nit 1.

**Cancellation.** No new preparation stage: the unit pass is per sample
inside the loop. DECLARED before measuring: a cancel returns within 4096 ROOT
samples once the loop has begun. MEASURED: see the 13A-3c verification
record.

**Review nits closed (from 13A-3b-ii).** (1) Named in `regional.rs`'s STATED
DIFFERENCES: a visible zero-opacity isolated group MEMBER of an
unmaterialized unit is a clamp-only step never rendered, so its
`rendered_group` charge (which the reference pays, and `unit_charges` pays
inside a materialized nest) is not dry-run at top level. (2) The `Styled` doc
says "a raster, or a materialized group's synthetic raster". (3)
`precision_regional_tree.rs`'s module doc is rewrapped. `make test` passed in
full after the three nits and before any behaviour change — the byte-identity
check (see the verification record).

## 13A-4a contract deltas

13A-4a extends the 13A-3c contract; everything above not restated here
holds. `core/src/regional.rs`'s module doc is still the contract's home;
`core/src/regional_guide.rs`'s module doc holds the geometry and the
exactness argument.

**Admitted set.** 13A-3c's set plus a Shadows/Highlights whose guide is read
(`Adjustment::is_pointwise` false) at the ROOT level, at 8 and 16 bits: a
lone entry or a baseless clipped entry (8 bits: a base whose POSITIONAL run
is empty — `composite_level_into`'s `&[]`; typed: the lone arm, which a
hidden-only run takes), masked (feather 0 and 6), at any opacity, under any
blend mode the depth admits (Multiply and Dissolve tested), above a knockout
Background (D5), two or more of them in one stack, and over empty isolated
groups. A guided BASE over a non-empty run contributes nothing (its clip
buffer is transparent, so `adjust_backdrop_sample` returns at once) and keeps
its ordinary row (no stage, no guide halo). A guided Shadows/Highlights inside a
group, as a clip member or inside a materialized subtree still REFUSES "is
Shadows/Highlights with a guide" (13A-4b). Float carries no guided operator
(the float policy refuses it with its own sentence at the full render, the
plan and the render — asserted). Under 11E the policy refuses a guided
Shadows/Highlights (`doc_evaluation::REFUSED`), so `transfer` is `None`
wherever a guide is admitted (`with_evaluation(LinearLight)` is `Err` at both
depths, asserted) — no policy-table change.

**Tolerance.** EXACT at both depths, fixed before measuring:
`adjust_guide::build` builds the guide cell-wise on a lattice fixed in
accumulator coordinates, `plane_blur` is bit-identical on a sub-plane on the
`SUB_PLANE_ALIGN` lattice whose reads lie ≥ `blur_reach` inside it (8 bits),
`gaussian_plane::feather` is a direct clamped convolution with half ≤
`blur_reach` (typed), and `GuidePlane::at` reads ±1 cell — so a plane equal
to the accumulator over `plane_region(R_k)` yields the reference's guide bit
for bit at every sample of `R_k`.

**Stages.** With `m` guided root rows, planes are chosen from the LAST guide
backwards: `R_m` = the request window, `R_{k−1}` = `plane_region_k(R_k)` —
`R_k` grown by `factor · reach_cells` (`reach_cells = blur_reach(grid_sigma)
+ 2`), origin rounded DOWN and end UP to multiples of `factor ·
SUB_PLANE_ALIGN`, clamped to the canvas. Stage 0 fills `P_0` over `R_0` at
divisor 1 with the units below the first guide; stage k builds guide k from
`P_{k−1}` with the reference's own builder (`Adjustment::guide_with_job` →
`tone_plane_with_job`; typed `PrecisionAdjustment::guide` under
`progress.job.range`, `render_layers`' order), hands it to the unit, and
evaluates the units up to the next guide over `R_k` (divisor 1) or — the last
— over the OUTPUT at the request's sampling. D5's Background split runs in
stage 0; its snapshot `S` is held over `R_0` and read by every later unit.
`Stage.layer` and the walks' `guided` are LAYER indices (byte `Node.at.idx`,
typed record index); only `ResolvedLevel::unit_of_layer` maps them to
resolved positions, because `resolve` drops a unit whose base resolves to
`None` (an empty isolated group) — the byte driver refuses `None` or a
non-increasing mapping ("regional stage mismatch", defensive), the typed
driver a count mismatch between the root's guided nodes and the plan's
stages.

**Plan.** `contract_version` stays 1. New top-level `"stages": [{"layer",
"plane": {x, y, width, height}, "cell_pixels", "reach_cells"}, …]` (empty
without a guide) and `"border_policy": "canvas_clamped"` (a plane meets the
reference's own edge handling at the canvas edge and nowhere else). A guided
row is `{"kind": "spatial", "halo_pixels": max(mask pad, factor ·
(reach_cells + SUB_PLANE_ALIGN))}`. The top-level `halo_pixels` is
`regional_guide::input_halo`: the max over rows of the row's own halo plus
the halos of every stage STRICTLY ABOVE it (a row below a guide is evaluated
over that guide's plane) — the largest row halo with no stage, so every
earlier plan restatement holds — and `required_input_region` (the window
grown by it) contains `stages[0].plane`. Scratch adds `Σ_{k<m} |R_k|·16`, each
guide's `adjust_precision_plan::guide(R_{k−1})` scratch (the f32 guide's
figure — conservative at 8 bits, whose guide stores bytes) and, under D5,
`|R_0|·16`. Work: byte `Σ_{k=0..m} |R_k|·rows_k + Σ_k guide work + pixels`
(`R_m` = the output; `m = 0` is `pixels·(rows + 1)`); typed the same plus
today's mask / style / materialization terms plus `|R_0|` (`P_0`'s
`zero_plane` fill advances it). STATED COST: a preview below a guided row
still evaluates its stages at divisor 1 — no resolution cut. The declared
halo is a contract figure only; no buffer is sized from it. FFI signatures,
Swift and the MCP `render` arguments are unchanged.

**Refusal order** unchanged: (1) the unit skip; (2) entry by entry, a
group's own level or the operator — now only a guided Shadows/Highlights
inside a group or as a clip member refuses, where the operator was checked.
The six styled / group files' order tests and `regional_byte_effects.rs` /
`regional_masks_clips.rs` / `regional_byte_styles.rs` keep passing; only the
two lone-root tests (`regional_document`, `regional_precision`) flipped to
admission-equals-reference.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096
samples of the running stage loop, or after the running byte `blur_plane`
(uncancellable, 13A-5) completes. MEASURED: see the 13A-4a verification
record.

**Review nits closed (from 13A-3c).** (1) `regional_knockout::
base_only_backdrop` is the one construction of the six member loops'
base-only backdrop. (2) `regional_knockout.rs` mixes by `s * opacity` (the
opacity arrives sanitized). (3) `ResolvedStep::wants_base`,
`ResolvedNode::is_shaped` / `footprint_limit` and `isolated_env` moved into
`regional_knockout.rs`; the new 13A-4a code lives in new modules. `make test`
passed in full after the nits and again after the guide hoists, before any
behaviour change — the byte-identity checks (see the verification record).

## 13A-4b-i contract deltas

13A-4b-i extends the 13A-4a contract; everything above not restated here
holds. `core/src/regional.rs`'s module doc (NESTED GUIDES) is still the
contract's home; `core/src/regional_guide.rs`'s module doc (LEVELS) holds the
geometry.

**Admitted set.** 13A-4a's set plus a guided Shadows/Highlights that is a
LONE unit (8 bits: no positional run; typed: no visible member) of an
ISOLATED group's level at 8 and 16 bits — nested to the depth cap, at any
unit position, with root guides below and above it, two in one level, the
group lone, a clip base with a visible member, a clip member, inside plain
and gated (opacity / mask) pass-through groups, masked (feather 6) at opacity
0.7 Multiply, or carrying a fill / Blend If style — and a guided
Shadows/Highlights ANYWHERE below a MATERIALIZED group (an isolated group
whose style has an enabled effect), in isolated and pass-through levels
alike: the reference renders that subtree (`rendered_group` / `render_layers`
over `render_styled`'s buffer), so it is admitted with no stage, its row
declaring the guide's halo. A guided base over a non-empty run inside a group
contributes nothing (ordinary row, no stage). STILL REFUSED "is
Shadows/Highlights with a guide" (13A-4b-ii): a guided CLIP MEMBER — inside
materialized subtrees too — and a guided entry directly inside a PASS-THROUGH
level (plain or gated). Refusal order and sentence are unchanged.

**Tolerance.** EXACT at both depths, fixed before measuring. The byte
reference builds a nested guide from the GROUP's buffer
(`doc_group::rendered_group` over extent ∩ clip at origin (x0, y0) →
`doc::composite_adjustment_into` with the buffer's `acc_w/acc_h`, read at
buffer-local `(ax, ay)`), so the cell lattice is buffer-anchored and
`blur_plane` clamps at the buffer's edge; the typed `render_layers`' Group arm
renders children into a canvas-sized `zero_plane`, so the buffer is the
canvas. 13A-4a's argument holds with `buffer` in place of the canvas.

**Levels.** The walks thread `regional_guide::Enclosure` (`Root`,
`Isolated`, `PassThrough`, `Materialized`), computed for a child BEFORE its
level is walked: `Materialized` when the enclosing one is (STICKY) or the
child materializes, else `Isolated` / `PassThrough` by the reference's
isolation (byte `is_isolated` or a member; typed `forced` or a `Group`). Each
non-materialized level returns a `LevelRecord` (its group, its LAYER range,
its `buffer` — byte: the group's rect, `None` when empty; the enclosing one
for pass-through; the canvas at the root and at every typed level — its
staged guided rows, and its Isolated / PassThrough children with the number
of guided rows below each). `regional_guide::plan_levels` walks that tree:
`R_m = target ∩ buffer` (at the root the request window; empty → nothing for
the level and its subtree), `R_{k−1} = plane_region_k(R_k, buffer)` —
`GuideGeometry::plane_region` now grows and aligns relative to the BUFFER's
origin and clamps to it (the canvas at the root: 13A-4a's formula) — and each
child is planned with the target its unit is evaluated over
(`stages[count].plane` below this level's last guide, else `R_m`). A level
EMITS a `StagedLevel` (pre-order) only when it holds a guided row; an
unguided level recurses only. `RegionalPlan.levels` replaces `stages`;
`root()` is `levels[0]` iff its `level` is `None`, `nested()` the rest.

**Render.** After `resolve` / `prepare` and BEFORE the root loop,
`stage_nested` walks the tree: each unit is evaluated over
`mine.stages[k].plane` (k = guided lone units below it) or the target; a
pass-through child recurses over the same region with no level; a
materialized child is never entered; an ISOLATED child — byte: over `R =
region ∩ rect` (empty → skipped entirely), its `StagedLevel` looked up by the
group's index; typed: its level POPPED from `plan.nested()` BEFORE recursing
(so the pops follow the plan's pre-order) — recurses, then renders its staged
level (`render_staged` with `Last::Region(target)` under
`KnockoutBackdrop::NONE`: every stage at divisor 1, the LAST plane held over
`target`) and replaces its children by that plane (byte `Source::Staged(rect,
StagedPlane)`, sampled as the group's synthetic pixel through
`encode_sample`; typed `Node.staged`, read by the Group arm and the unstyled
styled-group arm). A target that disagrees with the region, a missing level,
or a leftover pending level is `"regional stage mismatch"` (defensive). The
root then runs 13A-4a's staged loop (`Last::Output`) or the ordinary loop;
the byte `done` is shared across nested and root calls so the reported
fraction stays monotonic (the ordinary loop reports against `work_units`
when a nested level exists, and exactly as before otherwise).

**Plan.** `contract_version` stays 1. Each `"stages"` entry gains `"level"`
(null at the root, else the group's index) and `"buffer"` ({x, y, width,
height}: the canvas at the root and at 16 bits, the group's rect at 8 bits);
the list is flat, level by level in pre-order, bottom-first within a level.
`border_policy` stays `"canvas_clamped"` (each stage's `buffer` names its
clamp rect). A nested level adds scratch `Σ_{k≤m} |R_k|·16` plus its guides'
and work `Σ_{k≤m} |R_k|·rows_k` plus its guides' (`rows_k` the plan rows
with index in `[g_k, g_{k+1})` — descendants included, so an enclosing level
still counts them over its own plane: conservative, `report` clamps); typed
work also adds each staged level's `P_0` fill. `input_halo` sums the halos of
the stages of EVERY level above a row (conservative across sibling levels),
so `required_input_region` contains every plane. STATED COST: a nested
level's LAST stage is at divisor 1 over window ∩ buffer even for a preview
(only the root's last stage samples the output). FFI signatures, Swift and
the MCP arguments are unchanged; the header comment, the MCP catalog's
`region` description, README and `core/CLAUDE.md` name the narrowed refusal.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096
samples of the running stage loop (nested included) or after the running byte
`blur_plane` / `rendered_group` (uncancellable, 13A-5) completes. MEASURED:
see the 13A-4b-i verification record.

**Review nits closed (from 13A-4a).** (1) The byte per-sample bodies
(`ResolvedLevel::sample` / `sample_units`, `ResolvedUnit::sample`) moved
verbatim to `doc_regional_sample.rs` and the typed styled bodies
(`styled_sample16` / `direct_sample16`) to `precision_regional_style.rs`; the
trees end at 496 / 446 lines. (2) The 8-bit final-stage cancel estimate is
calibrated on the final stage's own uncancelled loop (the u16 twin's
method). `make test` passed in full after both, before any behaviour change —
the byte-identity check.

## 13A-4b-ii-a contract deltas

13A-4b-ii-a extends the 13A-4b-i contract; everything above not restated
here holds. `core/src/regional.rs`'s module doc (PASS-THROUGH GUIDES) is
still the contract's home; `core/src/regional_guide.rs`'s module doc
(PASS-THROUGH) holds the geometry and the hold rule.

**Admitted set.** 13A-4b-i's set plus a guided Shadows/Highlights that is a
LONE unit (8 bits: no positional run; typed: no visible member) directly
inside a PASS-THROUGH group at 8 and 16 bits — plain, gated by opacity 0.6,
by a feathered (6) mask or by both, pass-through groups nested (plain in
gated, gated in plain, both gated), at the root or inside an isolated group's
level, beside a Shallow knockout child, under D5's Background, between root
guides below and above, two in one pass-through, and an isolated group whose
ONLY guide sits inside its pass-through child. A pass-through clip base with
hidden-only members stays pass-through (absorbed); with a VISIBLE member it is
isolated at both depths (the typed scene builds it as `isolated_normal`,
`Kind::Group`) and staged as its own level, as before. STILL REFUSED "is
Shadows/Highlights with a guide" (13A-4b-ii-b): a guided CLIP MEMBER, anywhere
— inside a pass-through group and inside materialized subtrees included.
Refusal order and sentence are unchanged; only the refusal condition
narrowed (`clip_base.is_some()` alone).

**Tolerance.** EXACT at both depths, fixed before measuring. A plain
pass-through's children composite straight onto the enclosing accumulator
(`doc_group::composite_unit_into`'s pass-through branch), a gated one's onto
`acc.to_vec()` (`composite_gated_pass`), the typed arm's always onto a
canvas-sized copy (`precision_render::render_layers`' `PassThrough` arm) — a
buffer with the ENCLOSING buffer's size and origin — and the reference builds
the guide from that buffer, so the enclosing level's lattice and clamp apply
and 13A-4b-i's argument holds unchanged.

**Levels.** A pass-through level is never staged on its own: the walks
ABSORB its record into the enclosing Root / Isolated record
(`LevelRecord::absorb` — its isolated children appended with their `below`
counts shifted by the enclosing record's guided rows so far, its guided rows
appended with `Through { group, held }` inserted at the front of each row's
`through`). `held` is byte `pass_through_holds` — the gate (opacity below 1
or an enabled mask) or a visible Shallow child of the group's own level
(`level_wants_shallow`), exactly the reasons `composite_unit_into` reads the
sample as the children began — and typed always (the typed arm always
copies). A Materialized child is still dropped, an Isolated one still pushed.

**Render.** Each guided unit is reached by a CURSOR: unit positions from the
staged level down through pass-through children (byte
`ResolvedLevel::path_of_layer` from the stage's layer index; typed
`guided_paths`, whose count must equal the level's stages and whose paths
must end at an adjustment — else `"regional stage mismatch"`, as must a
non-increasing byte path or D5 with `paths[0][0] == 0`). Every FILL runs
`sample_between(from, to)`: P_0 is `([], paths[0])` over `R_0` (D5: `([],
[1])` then `([1], paths[0])`, both over `R_0`), loop k is `(paths[k],
paths[k+1] or [])` over `R_{k+1}`, a nested level's held target, or the
OUTPUT. A cursor's unit `from[0]` (with more path) is RESUMED — its child
level from `from[1..]`, Shallow the held sample — and, unless `to` stays in
it, FINISHED (`a = S; gate(&mut a, acc, coverage·opacity); acc = a` — byte
`gate_source_into` only when gated, typed `gate_pass_through_sample16`
always); `to[0]` (with more path) is ENTERED — the sample written to its hold,
then its children from the start with Shallow the sample as it stood. The
stage plane therefore holds the working sample at the cursor (the enclosing
accumulator, or the gated copy, which starts equal to it). HOLD SCHEDULE:
immediately before each fill, every proper prefix of `to` that is not a
proper prefix of `from` and is held gets a plane over THAT fill's region
(never the output: `to` is empty there), filled by the chunked byte `plane` /
typed `zero_plane`; `Holds` lives for the whole `render_staged` call, so a
hold entered in P_0 is read by the FINISH in the last loop. `stage_nested`
threads `passed` (the guided units of the staged level passed so far) through
pass-through children with the SAME staged level and target, and an isolated
child is staged when its level holds a guided unit ANYWHERE on a path (byte
`ResolvedLevel::guided_paths`, typed `guided_paths` — the typed `pending`
pop follows it), so an isolated group whose only guide sits inside a
pass-through child is staged like any other.

**Plan.** `contract_version` stays 1. Each `"stages"` entry gains
`"through"`: the pass-through groups between the level and the row,
outermost first (`[]` for a level's own row). Geometry is unchanged (a
pass-through row is a stage of its enclosing Root / Isolated level, planes
anchored at that level's buffer). Each distinct HELD group over the level's
stages' `through` adds scratch `16·|stages[0].plane|` and work
`|stages[0].plane|` (the fill; typed `zero_plane` advances it, the byte
`plane` only polls) — conservative: a hold entered by a later loop lives over
`R_{k+1} ⊆ R_0`, and a hold is never dropped after its last FINISH. FFI
signatures, Swift and the MCP arguments are unchanged; the header comment,
the MCP catalog's `region` description, README and `core/CLAUDE.md` name the
narrowed refusal.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096
samples of the running stage loop (nested and pass-through paths and hold
fills included), or after the running byte `blur_plane` / `rendered_group`
(uncancellable, 13A-5) completes. MEASURED: see the 13A-4b-ii-a verification
record.

**Carried items closed.** (1) `regional.rs` restates the bound (both places)
as above. (2) The byte walk drops the member-group records of a unit whose
base group has an empty extent (`record.children.truncate(mark)`), so the plan
no longer emits or charges a staged level the render never stages. (3) The
walks moved VERBATIM to `doc_regional_walk.rs` / `precision_regional_walk.rs`
(`doc_regional.rs` 330, `precision_regional.rs` 251 lines); `make test`
passed in full after the move, before any behaviour change — the
byte-identity check.

## 13A-4b-ii-b-1 contract deltas

13A-4b-ii-b-1 extends the 13A-4b-ii-a contract; everything above not
restated here holds. `core/src/regional.rs`'s module doc (CLIP-MEMBER GUIDES)
is the contract's home; `core/src/regional_guide.rs`'s module doc (CLIP
UNITS) holds the hold and row rule.

**Admitted set.** 13A-4b-ii-a's set plus a guided Shadows/Highlights that is
a CLIP MEMBER of an UNSTYLED base (8 bits: `style_of(base)` is `None`; typed:
the base record carries no style — a typed Blend If base is admitted, its
members clamped by `min(a)`) at 8 and 16 bits: clipped to a holey raster at
the root (radii 4, 30, 120), with raster members below and above it, two
guides in one unit, beside a Shallow knockout member (below and above it),
masked (feather 6) at 0.7 Multiply, beside hidden (dropped) and zero-opacity
(8 bits clamp-only, typed kept) raster members, clipped to an ISOLATED group
base, the unit inside an isolated level, inside plain, gated and nested
pass-through groups, between root guides below and above, under D5's
Background, and clipped to an adjustment base (contributes nothing, still
staged). The guided member itself hidden is dropped (no stage); at opacity 0
it is a clamp-only step at 8 bits (no stage) and kept by the typed scene
(staged, then skipped by the whole-layer skip). Inside a MATERIALIZED subtree
a guided member is admitted unstaged (the reference renders it; the guide's
halo declared). A byte member under an EMPTY base group is dropped with its
unit (no stage). REFUSED, at plan and render: a guided member whose base is
STYLED — fill-only, effects, knockout, a materialized group base — with the
new sentence "regional render refuses layer N (name): is Shadows/Highlights
with a guide under a styled base" (13A-4b-ii-b-2's). The old "is
Shadows/Highlights with a guide" sentence is gone. Refusal ORDER is unchanged:
the member is checked in `level()` right after its row is built, where
`node()` / `row()` refused before; `Adjustment::is_pointwise` stays the
predicate.

**Tolerance.** EXACT at both depths, fixed before measuring. The reference
builds the member's guide from the unit's PRIVATE buffer: at 8 bits
`composite_clip_group_into`'s `vec![[0.0; 4]; acc.len()]` (`doc.rs:579`),
accumulator-aligned; typed a `zero_plane(acc.len())` (`precision_render.rs:866`),
canvas-sized — the ENCLOSING level's size and origin either way, so the
level's lattice and clamp apply and 13A-4b-i's argument holds unchanged.

**Render.** CURSORS gain a terminal MEMBER STEP: unit positions down through
pass-through bases, optionally ending in the member step of the last unit,
which is then a CLIP unit (byte `clip_run` over a non-pass-through base, the
step index counting clamp-only steps; typed `full` over non-empty `members`);
an element after a pass-through unit is a child unit, after any other unit a
member step. Byte `path_of_layer` / `guided_paths` / `node_at(_mut)` and the
typed `guided_paths` / `node_at_mut` reach member steps (`guided_member`: an
adjustment with a guide radius over an unstyled base — the walk's staged
predicate; member paths after the base's own subtree paths); `clip_unit_at`
answers the new cursor element. `sample_between`: a clip unit ENTERED (`to`
ends in `[u, m]`) writes the sample as the unit began to its hold, then leaves
the unit BUFFER at member `m` in the working sample (the stage plane — the
guide's input only); RESUMED (`from` ends in `[u, m]`) it RE-RUNS the unit
from the held began sample — the base, then every member with its guide set —
to the next member cursor in it (`to[0] == u`) or its FINISH (`acc = began`,
then the buffer composited with the base's mode and opacity). The statements
are the ordinary clip-unit bodies EXTRACTED (byte
`ResolvedUnit::sample_clip_until`, typed `clip_unit_sample`), ordinary
sampling passing `under = acc`, `stop = None` (the reset a no-op). A missing
hold is a MISMATCH at typed depth and a `debug_assert!` + return at 8 bits.
HOLD SCHEDULE: byte `enter_holds` enters a proper prefix `p` of `to` when not
resumed and `snapshot_at(p)` or (`p` is `to` minus its member step and
`clip_unit_at(p)`); typed unchanged (every prefix). `stage_nested` (byte) /
`stage_node` (typed) recompute each member step's region from `passed` and
advance `passed` after a guided member.

**Plan.** `contract_version` stays 1 and no JSON key is added
(`layers[].clip_base` already names the base). `GuidedRow` / `Stage` carry
`clip_unit: Option<ClipUnit { base, start }>` (byte `Unit.idx` /
`Unit.start`, typed `index_of(i)` / `starts[i]`). THE MEMBER-STAGE ROW RULE:
the lower bound of stage `k ≥ 1` is `guided[k-1].clip_unit.map_or(g_k,
|u| u.start)`, so a member stage's `rows` count from its unit's FIRST row —
every sample of its fill re-runs the unit from the base, so those rows are
charged (a raster base B at row 0 with a guided member M at row 1 at the
root: `rows_0 = 1`, `stages[0].rows = 2`, work `|R_0|·1 + pixels·2 + gw +
pixels + |R_0|`; a group base with N children adds N to every member stage;
the second of two guides in one unit counts the base, both members and
everything between); `rows_0` is unchanged. Each distinct clip unit holding
a guided member charges one more plane over `R_0` (scratch `16·|R_0|`, work
`|R_0|`), keyed by the base's index beside the held pass-through groups.
Progress needs no change (`evaluate` / `render_staged` advance `stage.rows`
per sample). The cost stays CONSERVATIVE, never under.

**Moves (byte-identity first).** `RegionRequest`, `EvaluationNeed`,
`LayerNeed`, `RegionalPlan` (+ `json`), `STYLE_PLANES_ANALYSIS`,
`MATERIALIZATION_ANALYSIS` moved VERBATIM to `regional_plan.rs` (re-exported
by `regional`); the byte cursor block (`path_of_layer` … `snapshot_at`) to
`doc_regional_sample.rs` (`ResolvedLevel.layer_units` became `pub(crate)` so
the moved block compiles); the typed `sample_between`, `guided`,
`guided_paths`, `node_at_mut` to the new `precision_regional_sample.rs`
(`pass_through_env` / `finish_pass_through` `pub(crate)`); `intersect` /
`area` to `regional_plan.rs` (`regional_guide` re-exports `intersect`).
`make test` passed in full after the moves, before any behaviour change.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096
samples of the running stage loop (clip-unit re-runs and hold fills
included), or after the running byte `blur_plane` / `rendered_group`
(uncancellable, 13A-5) completes. MEASURED: see the 13A-4b-ii-b-1
verification record.

**Carried items closed.** (1) `doc_regional_sample.rs`: a gated pass-through
read back without its hold now trips `debug_assert!(!gated || held.is_some()
|| entered == Some(u))` (the typed twin raises MISMATCH). (2) `regional.rs`
is below 600 lines (the plan types moved). (3) The byte PACKAGE path is
DECIDED: refused by name, owner 13B (Decisions).

## 13A-4b-ii-b-2 contract deltas

13A-4b-ii-b-2 extends the 13A-4b-ii-b-1 contract; everything above not
restated here holds. `core/src/regional.rs`'s module doc (STYLED-BASE MEMBER
GUIDES) is the contract's home; `core/src/regional_guide.rs`'s CLIP UNITS
paragraph holds the hold and row rule (unchanged, styled bases included).

**Admitted set.** 13A-4b-ii-b-1's set plus a guided Shadows/Highlights that is
a CLIP MEMBER of a STYLED base at 8 and 16 bits, EXACT: a fill-only base
(`{"fill_opacity":0.5}`, and with a Blend If at 8 bits — at 8 bits a style
without planes and unrouted still takes the unstyled private-buffer body,
which already is the styled-direct loop without planes), an effects base
(drop shadow + stroke) at opacity 1, an effects base below opacity 1 in
Dissolve (never packaged), a Shallow knockout base without planes and a
knockout base with effects at opacity 1 (both ROUTED), Deep / Shallow
knockout bases under D5's Background, a MATERIALIZED group base (effects on
an isolated group); with raster members below and above the guide, two
guides in one unit, a Shallow knockout MEMBER beside the guide, the member
masked (feather 6) at 0.7 Multiply, a knockout base whose layer rect is a
small patch inside a much larger window, the unit inside an isolated level
and inside plain and gated pass-through groups. At 16 bits the PACKAGE too
(effects below opacity 1, not Dissolve — a raster base and a materialized
group base at 0.85): the typed package is ONE canvas-sized copy. REFUSED, at
plan and render, 8 bits only: a guided member whose base is PACKAGED
(`Styled::of`'s `package`: the style renders planes, `sane_opacity < 1`, not
Dissolve — a materialized group's synthetic keeps the group's opacity and
blend) with the new sentence "regional render refuses layer N (name): is
Shadows/Highlights with a guide under a packaged styled base (8-bit effects
below opacity 1)" (owner 13B). The 13A-4b-ii-b-1 sentence "… under a styled
base" is gone. Refusal ORDER is unchanged (the member check in `level()`
right after its row).

**Tolerance.** EXACT at both depths, fixed before measuring. The reference
builds the member's guide from the STYLE's private unit buffer — the
styled-direct member loop (`style_composite.rs:422-436`, typed
`precision_style_render::composite_direct_into`) or the routed unit
(`style_knockout.rs:660-682`, typed `precision_style_knockout`) —
accumulator-sized at 8 bits and canvas-sized at typed depth (the typed
package's copy included), so the level's lattice and clamp apply and
13A-4b-i's argument holds unchanged. Outside the layer rect that buffer is
the members composited over transparency and clamped to the footprint's 0,
and the guide reads it there too.

**Render.** The staged cursor's member index is a STOP (`Some(m)` only from
`sample_between`'s ENTER / RESUME-to-member; `None` everywhere else). Byte
`styled_sample` / `direct_sample` and typed `styled_sample16` /
`direct_sample16` gain `stop`: the Below contributions run (they land on the
target and are overwritten), then the clip branch's members `[..m]` or the
unit pass's, the private buffer is left in the target sample and the body
returns BEFORE `composite_source_into` / `K` and the Interior contributions.
`unit_pass_sample` / `unit_pass_sample16` keep ONE body with TWO orders gated
on `stop`: under `None` the early returns (outside the rect, `s ≤ 0`) come
first, statement for statement as before; under `Some` the unit is computed
at every sample (the pixel pass only under `Some(px) && s > 0 && fill_in >
0`), the members truncated and the unit left in the target; a grouped
interior never meets a stop (`grouped_interior`'s narrowing, a
`debug_assert!`). Byte `styled_sample` with a stop under the package is
unreachable (`debug_assert!` + return); typed `styled_sample16` with a stop
under the package runs `direct_sample16` on the copy at opacity 1 and stores
the copy, with no window test and no mix. DISPATCH: byte
`ResolvedUnit::sample` routes EVERY clip run through `sample_clip_until`,
whose first statement sends a base with planes or a routed style to
`styled_sample(.., Some(steps), .., stop)` after `acc[ai] = under` (a
fill-only base keeps the existing body); typed `clip_unit_sample` sends any
styled node to `styled_sample16(.., stop)` after `acc[i] = under`;
`sample_layers` and the byte styled MEMBER path pass `None`. PREDICATES: byte
`guided_member` drops the base-style test; typed `guided_member` is deleted
(its callers use `guided`) and `clip_unit_at` admits a styled node with
members — so `path_of_layer`, `guided_paths`, `enter_holds`, `stage_nested` /
`stage_node` reach members of styled bases with no other change.

**Plan.** Unchanged: `contract_version` 1, no key added; the member-stage row
rule, the one hold per distinct clip unit and the D5 snapshot as in
13A-4b-ii-b-1. A styled base's own scratch (planes, materialization) and the
typed plan's preparation work are unchanged by the guide.

**Moves (byte-identity first).** `plan_levels`, `plan_level` and
`input_halo` moved VERBATIM from `regional_guide.rs` to `regional_plan.rs`
(re-exported by `regional_guide`); `materialization_charges`,
`level_charges`, `unit_charges` moved VERBATIM from `doc_regional_style.rs`
to the new `doc_regional_budget.rs`. `make test` passed in full after the
moves (2564 passed, the 13A-4b-ii-b-1 final count), before any behaviour
change. `regional_guide.rs` is 391 lines, `doc_regional_style.rs` 444.

**Cancellation.** DECLARED before measuring: a cancel returns within 4096
samples of the running stage loop (clip-unit re-runs and hold fills
included), or after the running byte `blur_plane` / `rendered_group` /
`LayerStyle::rendered` (uncancellable, 13A-5) completes. MEASURED: see the
13A-4b-ii-b-2 verification record.

**Carried item closed.** `regional_guide.rs` (603 lines) is below 600 (the
planner moved to `regional_plan.rs`).

## 13A-5a contract deltas

13A-5a extends the 13A-4b-ii-b-2 contract; everything above not restated
here holds. `core/src/regional.rs`'s module doc (CANCELLATION MODEL) is the
contract's home. No FFI, header, Swift-wrapper, plan-JSON (`contract_version`
1, `cancellation_checkpoint_pixels` unchanged), `.rz`, tool-table or 11E
change; refusals and the `work_units` formulas are untouched.

**The model (declared before measuring).** Every stage `s` declares a
CHECKPOINT INTERVAL `W_s` in its own units, and the render one RETURN FLOOR
`T_f` = 1 ms (unwinding from a checkpoint to the FFI return: dropping the
live planes, the join, the poller's 100 µs sleep). A cancel issued in stage
`s` returns within `T_f + W_s · c_s`, `c_s` the stage's per-unit cost
calibrated on its OWN uncancelled run on this machine. A measurement reports
the latency, `c_s`, the bound, `max(0, latency − T_f) / c_s` ("units past the
floor" — replacing `latency / c_s`, which divided the floor by a 0.3 ns fill
sample and misreported every hold fill since 13A-4b-ii-a) and PASS/MISS.
Intervals: sample loops and hold fills 4096 samples (`JobContext::every`,
the hold `plane`); the byte guide build 4096 cells (`build`'s `every`; its
grid blur's `feather` polls per 4096 cells and per row); the byte mask blur
at factor 1 (sigma ≤ 3, feather ≤ 8.33) 4096 cells per `feather` pass, at
factor > 1 ONE Triangle resample of the extended plane
(`image::imageops::resize`, polled only at its boundaries — replacing it
would change the mask's bytes), its cost timed by the test itself on the
same dimensions and filter (the larger of the down and up passes).

**Checkpoints.** `JobContext::checkpoints_only` (the same checkpoints, every
report at the range's start); `plane_blur::bytes_with_job` (`blur(.., job)`
verbatim, the byte twin of `floats`; `bytes` stays `blur(.., NONE)`);
`style_render::blur_plane_with_job`; `MaskProperties::derived_with_job` —
`derived`'s body with the polled blur, an `Err` writing NO cache entry, and
`derived` its `JobContext::NONE` call; the u8 `GuideSample::blur` hands the
job it is given to `blur_plane_with_job` (symmetric with the f32 impl). The
regional render polls three `feather`s under a live job, and all three take
`checkpoints_only()` at the CALL SITE — the two `derived_with_job` calls
(`Node::resolve`, `resolve_materialized`; `Level::resolve` / `Node::resolve`
/ `resolve_materialized` take the render's `job`) and the byte
`guide_with_job` call in `render_staged` — because `feather` reports
`0.5·(y+1)/h` and `0.5 + 0.5·(y+1)/h` per row and the token is `fetch_max`:
under the root range its last row would pin progress at 1.0 for the rest of
the render. So no byte probe moves for the blurs, and the guide's one report
stays the `run.done` advance after the build.

**Progress.** A byte HOLD fill (`enter_holds`, which now takes the `Run`)
adds each 4096-sample chunk to `run.done` and reports `run.done / run.total`
at stage 6 (the typed `zero_plane` → `Progress::advance` twin); the stage-0
`prev` and D5 `snapshot` fills pass `None` (their work is not in the byte
plan; the hold's `|R_0|` is), and `run.done` never exceeds `run.total` (a
hold's region ⊆ `R_0`). The three byte 4b-ii probe tables are re-based: the
hold fill opens at its entering guide's report `(a0 + g0)/work` and closes at
`(a0 + g0 + a1)/work` over `a1` samples; every later probe gains `a1`.

**Still uncancellable (owner 13A-5b).** Byte `LayerStyle::rendered`
(`Styled::of`), `rendered_group` and the typed `render_effects`.

**Carried nits closed.** The 13A-4b-ii-b-2 state-table row reworded to its
contract (hold as the unit BEGAN, the stop inside the style's member loop,
FINISH re-running the full styled sample); README and the `render` catalog
entry say "below opacity 1 (not Dissolve)" (MCP reference regenerated);
`doc_regional_style.rs`'s packaged-base guard is `debug_assert!(false, …)`.

## 13A-5b-i contract deltas

13A-5b-i extends the 13A-5a contract; everything above not restated here
holds. `core/src/regional.rs`'s module doc (CANCELLATION MODEL) is the
contract's home. No FFI, header, Swift-wrapper, MCP, plan-JSON, `.rz` or
README change; refusals, progress points and the `work_units` formulas are
untouched (MCP `render` passes no job control, so nothing app-visible moves).

**Checkpoints.** `style_plane_ops.rs` holds, moved verbatim out of
`style_render.rs` (which re-exports them, so no importer changed), the
numerics and the plane primitives, plus `dilate_plane_with_job` /
`erode_plane_with_job` (`grow_mask` / `shrink_mask`'s no-op guard, then
`doc_select::offset_contour_with_context` — `offset_contour`'s body — so the
distance transform and its mapping poll). The nine `style_fx_*::render`s take
a `JobContext` and return `Result`: `job.check()?` before every primitive,
the blur/dilate/erode through their `_with_job` twins, and the bespoke
per-pixel loops polled once per ROW — the bevel's two height-field
saturating adds (`chunks_exact(w)` row loops), `shade` (per `y`) and its two
coverage maps (one row loop extending both), the gradient overlay's loop
(per `y`), the stroke's `gradient_fill` (per `y`), satin's soft XOR (a row
loop) — each rewrite the same arithmetic in the same order.
`render_effects_with_job` / `render_effect_with_job` dispatch;
`render_effects` / `render_effect` are their `JobContext::NONE` wrappers
(`.expect("uncancellable context")`). `LayerStyle::rendered_with_job` is
`rendered`'s body with `job.check()?` around `Shape::of_layer` and the job
through `settled` / `patched` (now answering `Patch::{Done, Full}`) /
`adopted`; every `Err` returns before any `store` and `settled`'s adoption is
built into a local before any entry is written — a cancelled render caches
nothing (the cache is an `Arc<LayerStyle>` every clone shares). `rendered`
is its NONE wrapper. `Styled::of` takes the render's `job` and returns
`Result`; it, `prepare_style` and `composite_styled_into` pass
`checkpoints_only()` at the CALL SITE (13A-5a's rule: `feather` reports
`0.5·(y+1)/h` per row and would pin the root range at 1.0). The typed style
`advance` stays its one report.

**Intervals (declared before measuring).** Inside a blur, 4096 cells per
`feather` pass at factor 1 (`feather` also polls per row; with `pw ≤ 4096` a
chunk never exceeds one row) and ONE Triangle resample of the extended
plane at factor > 1; inside a dilate/erode, the SDF's own polling and
`every(4096)` in the mapping; ONE ROW (`pw` cells) inside each bespoke loop
above; every other primitive ONE unit polled at its boundaries —
`Shape::of_layer`, `Shape::inverted`, the coverage `clone` / `to_vec`, the
all-zero scans, `shift_plane`, `multiply_planes`, `subtract_planes`,
`invert_plane`, `push_banded`'s multiply — each one memory-bound pass over
the plane, bounded by `c_map` (the test's own timing of one
`multiply_planes`-shaped pass over two `pw·ph` buffers). Bounds: factor 1
`T_f + max(4096·c, c_map)`, `c = t_style / (2·pw·ph)`; factor 8 `T_f +
max(c_resample, c_map)`; the bevel `T_f + max(t_style / ph, c_map)`.

**The guards (the carried should-fix).** `regional_byte_preparation_cancel.rs`
replaces the two first-observed-value guards with a REPORT LOG (`logged`:
every distinct progress value with the time it was seen; the logger reads
before testing `is_finished()` and once more after; `tf` is the WORKER's own
return stamp) and two signatures over it — INTERVAL (no logged value strictly
between the pre-stage value and the first legitimate report) and PIN (`tf −
t1 ≤ 0.5·t1`, `t1` when 1.0 was first logged: a leaked blur's last `1.0`
pins progress for the rest of the render). Neither depends on a short-lived
value; a logger stall can only delay `t1`, never inflate `tf − t1`.

**Still uncancellable (owner 13A-5b-ii).** Byte `rendered_group`.

## 13A-5b-ii-a contract deltas

13A-5b-ii-a extends the 13A-5b-i contract; everything above not restated
here holds. `core/src/regional.rs`'s module doc (MATERIALIZATION, SAMPLING,
CANCELLATION MODEL) is the contract's home. No FFI, header, Swift-wrapper,
MCP, plan-JSON, `.rz`, tool-table, 11E or README change; refusals, progress
points and the `work_units` formulas are untouched (MCP `render` passes no
job control, so nothing app-visible moves).

**The job in the env.** `CompositeEnv` gains `job: JobContext`. The two
literals (`RzDocument::composite_env`, the typed `byte_env`) set
`JobContext::NONE`, and every other construction spreads `..env`, so every
full-frame composite — `flattened` / `composite_projection_into`,
`merging_down`, `merge_layers`, `group_projection`, `inpaint_source`,
`merge_backdrop`, `doc_cutout_render`, `psd_export_rasterize` — carries NONE
and every `?` the chain gained is a never-taken branch there (`make test`
2605 before the new file: the byte-identity check). The ONE live entry is
`doc_regional_style::resolve_materialized`, which hands `rendered_group`
`CompositeEnv { job: job.checkpoints_only(), ..env }`, so the chain reports
nothing.

**Checkpoints.** `composite_layer_into`, `composite_adjustment_into` (now
taking the env), `composite_clip_group_into`, `composite_styled_into`,
`composite_direct_into`, `composite_package_into` and
`style_knockout::composite_interior_unit_into` return `Result<(), String>`.
Polls: `composite_layer_into` at entry and at the head of every `ay` row;
`composite_adjustment_into` at the head of every row, its guide through
`guide_with_job`; `composite_clip_group_into` before the buffer alloc, each
member and `composite_buffer_into`; `composite_styled_into` at each step
boundary (each Below contribution, the pixel pass / interior unit, each
member, `composite_buffer_into`, each Interior contribution; per package
STRIP before its copy and before its mix); the interior unit before its
alloc, its pixel pass, each grouped contribution, each member and at the
head of every row of its mix loop; `rendered_group` after `group_extent`,
after the buffer alloc and before `encode_projection`, its mask crop through
`effective_mask_rect_with_job`; `composite_level_into` at the top of each
iteration; `composite_unit_into` before the base, each `resolved_member` and
`composite_clip_group_into`, the pass-through mask through
`effective_mask_rect_with_job`; `composite_gated_pass` before `acc.to_vec()`
and per ROW of its lerp (rewritten as `chunks_exact_mut(acc_w)` with `i =
row·acc_w + x`, the same `gate_source_into` per pixel in the same order).
`merging_down` maps the three now-`Result` calls with `.ok()?`;
`doc_cutout_render` and `psd_export_rasterize` add `?`.

**Child masks.** `doc_mask.rs` adds `effective_mask_rect_with_job` (the
feather through `derived_with_job`, then one poll before the `from_fn`
crop), `aligned_mask_with_job` (the aligned cache stored only after the rect
returns `Ok`) and `active_mask_with_job` (`Ok(None)` unless enabled);
`effective_mask_rect` and `aligned_mask` are their NONE wrappers. The chain
reads a child's mask through `active_mask_with_job(job.checkpoints_only())`
at `composite_layer_into`'s raster branch, in `composite_adjustment_into`
and as a WARM at `composite_styled_into`'s entry, which precedes every
`active_mask()` reader of that layer (`Shape::of_layer`,
`composite_pixels_into`, the unit pass, the pad-0 shapes) — each later read
is an aligned-cache hit on the same `Arc`. The clip base's `full` clone and
`resolved_member`'s clone share the `Arc<Mutex>` caches, so
`composite_clip_group_into` needs no warm of its own. `Layer::active_mask`
is unchanged; `MaskProperties::derived` and `LayerStyle::rendered` are
uncalled and kept (`#[allow(dead_code)]`, 13A-5b-i's pattern).

**Intervals (declared before measuring).** Inside byte `rendered_group`:
ONE ROW of the layer rect within the buffer in `composite_layer_into`,
`composite_adjustment_into`, the knockout unit pass's mix loop and the gated
lerp; a nested guide build, a mask feather (the group's own AND every child
layer's enabled mask) and a style plane render each run their own 13A-5a /
13A-5b-i interval; every other primitive ONE unit polled at its boundaries
— the buffer alloc, `encode_projection`, the mask crops, `acc.to_vec()`
snapshots, the alpha resets, `composite_buffer_into`,
`composite_pixels_into`, `composite_contribution`, `clamp_alpha_to_shape`,
`Shape::of_layer`, `BackdropSnapshot::of`, a package strip's copy and mix —
each at most one pass over the buffer. Bounds: rows `T_f + c_row`, `c_row =
t_mat / ((N−1)·H)`; one-unit `T_f + c_unit`, `c_unit = max(t_mat / (N−1),
c_encode)`; a child mask blur `T_f + 4096·c`, `c = t_blur / (2·pw·ph)`; a
child style plane render 13A-5b-i's `T_f + max(4096·c, c_map)`; `T_f` = 1
ms. `t_mat = T(6) − T(1)` is the cost of N − 1 = 5 child composites.

**The carried probe fix.** `RzJobControl` counts its checkpoints
(`checkpoints: AtomicU64`, `fetch_add(1, Relaxed)` at the top of
`checkpoint()`; `#[doc(hidden)] pub fn checkpoints()`). The 13A-5a probes in
`regional_byte_preparation_cancel.rs` no longer cancel at a calibrated TIME:
(i) `cancelled_after_checkpoints` waits for the stage's entry, reads `n0`,
waits for `n0 + k` checkpoints and cancels; (ii) `cancelled_in_freeze`
cancels once the count has not advanced for 5 ms (the model's unpolled
unit). Both assert a verdict read from the worker's own state at its return
(progress, stage, `checkpoints ≥ n0 + k`) and fail naming "the poller
stalled past the stage", never "outside the target stage". Cost calibration
(`blur_window`, `guide_points`, `resample`) stays time-based.

**Still ONE unit (owner 13A-5b-ii-b).** `composite_pixels_into`,
`composite_contribution`, `clamp_alpha_to_shape`, `composite_buffer_into`,
`BackdropSnapshot::of`, the package strip mix, `encode_projection` /
`quantize`, `Shape::of_layer`, the mask crops.

## 13A-5b-ii-b contract deltas

13A-5b-ii-b extends the 13A-5b-ii-a contract; everything above not restated
here holds. `core/src/regional.rs`'s CANCELLATION MODEL paragraph is the
contract's home, with TWO edits: (i) the STYLE PLANE RENDER interval list —
`Shape::of_layer` leaves "every other primitive ONE unit polled at its
boundaries" and joins the ONE ROW list as "ONE ROW of the padded plane (`pw`
cells) in `Shape::of_layer`", bounded by `T_f + c_map` (no other interval of
that stage changes); (ii) the `rendered_group` paragraph — the intervals
below replace "every other primitive ONE unit … Still ONE unit (owner
13A-5b-ii-b)". `core/CLAUDE.md`'s `regional.rs` entry says the same. No FFI,
header, Swift-wrapper, MCP, plan-JSON, `.rz`, tool-table, 11E or README
change; refusals, progress points and the `work_units` formulas are
untouched (MCP `render` passes no job control).

**The primitives take a job.** `style_composite::composite_pixels_into`,
`composite_contribution`, `clamp_alpha_to_shape`, `BackdropSnapshot::of`,
`blend::composite_buffer_into` and `Shape::of_layer` gain a trailing `job:
JobContext` and return `Result` (`Shape::of_layer`: `Result<Option<Shape>,
String>`), each polling `job.check()?` at the head of every row of the loop
it already had (`ay` in the pixel / clamp / buffer loops, `py` in the
contribution loop, the copy loop of `of`, `y` in `of_layer`); the per-sample
statements (`composite_pixels_sample`, `composite_contribution_sample`,
`Shape::coverage_byte`) are unchanged and in the same order.
`composite_package_into` polls `env.job` per row of its strip COPY and MIX
loops. `doc::quantize_with_job` and `doc_evaluation::encode_projection_with_job`
are the per-row polled encoders (`w == 0 || h == 0` → an empty image before
any `chunks_exact`); `quantize` and `encode_projection` are their `NONE`
wrappers (13A-5b-i's job-less-name decision; `encode_projection`'s 9 callers
unchanged). `rendered_group` calls `encode_projection_with_job(.., env.job)`.
`effective_mask_rect_with_job`'s crop is an explicit row loop (`job.check()?`
per row, `effective_coverage_at` per sample in `from_fn`'s row-major order,
`GrayImage::from_raw`); the single pre-crop poll is gone. Byte call sites
pass `env.job` (inside `rendered_with_job`, its `job`); the SEVEN typed call
sites (`precision_style_render.rs`: the Blend If snapshot, two
`composite_contribution`s, `clamp_alpha_to_shape`, `composite_buffer_into`;
`precision_style_knockout.rs`: `composite_contribution`,
`clamp_alpha_to_shape`) pass `JobContext::NONE`, so the typed intervals stay
13A-5b-i's. At both Blend If snapshot sites the snapshot stays INSIDE the
filter's closure (`map(|b| Ok((b, rect.map(|r| of(.., job)).transpose()?))).transpose()?`),
so an absent or identity Blend If still copies nothing.

**The carried zero-width fix.** `composite_gated_pass` skips its lerp when
`acc_w == 0` (`chunks_exact_mut(0)` panics; the children still composite, and
the old `iter_mut().enumerate()` loop did nothing over an empty `acc`).

**Intervals (declared before measuring).** STYLE PLANE RENDER: ONE ROW of
the padded plane (`pw` cells) inside `Shape::of_layer`, bounded by `T_f +
c_map` (a row is a fraction of the one-pass `c_map`). Inside byte
`rendered_group`: ONE ROW of the buffer (or of the strip, plane, layer rect
or crop) inside `composite_pixels_into`, `composite_contribution`,
`clamp_alpha_to_shape`, `composite_buffer_into`, `BackdropSnapshot::of`, the
package strip's copy and mix, `encode_projection_with_job` /
`quantize_with_job`, `Shape::of_layer` and the mask crop (the aligned plane
is a move: `aligned_mask_with_job` returns its Borrowed case before
`into_owned`), bounded by `T_f + c_row`, `c_row = t_chain / n_chain` (the
chain's mean per-checkpoint cost over its own uncancelled run); still ONE
unit: the buffer allocs, the `acc.to_vec()` / `buf.clone()` snapshots, the
alpha scans and resets, the coverage clone and `drop(buf)` — each one
memory-bound pass, bounded by `T_f + c_copy`, `c_copy` the test's own timing
of one `to_vec()` of a W·H `[f32; 4]` buffer.

**Probes.** The checkpoint-trigger helpers moved to
`core/tests/common/probe.rs` (`RETURN_FLOOR_S`, `state`, `Trigger`, `Probe`,
`probe`, `cancelled_after_checkpoints`, `cancelled_in_freeze`, `at_the_blur`,
`inside_the_blur`, `inside_the_chain`, `checkpoints_line`, `verdict`, `past`;
`past_the_fill(w, h)` re-parameterized; plus `window`, the uncancelled
entry/exit calibration). `regional_style_preparation_cancel.rs`'s time
probe (`cancelled_at`) is gone: byte size 4 and the bevel cancel `k =
(n_exit − n_entry)/2` checkpoints after `at_the_blur` (the window from that
flip to the first report above 0), byte size 40 on a 5 ms freeze, verdict
`inside_the_blur`; u16 the entry `progress ≥ pre − 1e-6`, exit `progress >
pre + 1e-6`, `k` half the window (size 4) or the freeze (size 40), verdict
progress still at `pre`; the cache-integrity test the same triggers. The
materialization file's (m2) moves to (g′) — (g)'s children inside a GATED
(0.6) pass-through group inside the styled group, `composite_gated_pass`'s
unpolled `acc.to_vec()` — on a `min(5 ms, c_copy/2)` freeze, bound `T_f +
c_copy`; `c_encode` is gone.

## 13A-5b-iii-a contract deltas

13A-5b-iii-a extends the 13A-5b-ii-b contract; everything above not restated
here holds. `core/src/regional.rs`'s CANCELLATION MODEL paragraph is the
contract's home: its sentence "The two byte preparation blurs poll under
`JobContext::checkpoints_only` … a cancelled mask blur caches nothing." is
replaced by the statement below (13A-5a's interval sentence for the byte
guide build — 4096 cells, its grid blur's `feather` polling per 4096 cells
and per row — stays). `core/CLAUDE.md`'s `regional.rs` entry says the same.
No FFI, header, Swift, MCP, plan-JSON field, `.rz`, tool-table or 11E
change; the full-frame path is untouched (`adjust_guide.rs`,
`plane_blur.rs`, `gaussian_plane.rs` unchanged); every staged byte render's
samples are unchanged (the five guide files' and the preparation suite's
exactness tests).

**The model statement.** The byte mask blur polls under
`JobContext::checkpoints_only` (every report lands at the range's start):
the byte plan counts no mask work, so no progress point moves, and a
cancelled mask blur caches nothing. Since 13A-5b-iii-a the byte guide build
reports INSIDE ITS RANGE on the typed pattern (`job.range(done/total, (done
+ g)/total)`, `g` its plan work): flat at the range's start through
`adjust_guide::build`'s grid pass (its polls report nothing), the first grid
blur's `feather` rows ramp start → end (`plane_blur::blur` at the reduced
sigma; above sigma 3 its resamples and copies poll only and the reduced
plane's rows ramp), the second blur re-ramps under `fetch_max` and so holds
the end, as does the combine, then the driver's advance lands on it — the
typed guide has the same shape (per-blur sub-ranges inside `build` are
13A-5b-iii-c's); its interval stays 4096 cells. The byte stage-0 `prev` and
D5 `snapshot` fills advance and report per 4096-sample chunk (each `|R_0|`
in the byte plan's `work_units`, the typed `fill`'s twin; the typed snapshot
is pushed inside stage 0 and uncounted — a stated difference). The byte
style planes still poll under `checkpoints_only` and the unstaged
accumulator fill polls without reporting (owner 13A-5b-iii-b).

**The driver.** `doc_regional_guide::render_staged`: `prev = plane(|R_0|,
job, Some(&mut run))`, the D5 `snapshot` likewise; per stage `k`, `g =
adjust_precision::plan::guide(w, h, radius)?.1`, the guide built under
`job.range(done / total, (done + g) / total)`, then `done += g` and
`report(done / total, 6)` (the `precision_regional_guide` order).
`stage_nested` / `stage_node` are untouched (a nested `render_staged`
inherits both).

**The plan.** `regional_admission`'s byte `work_units` gains `fill =
Σ_levels area(level.stages[0].plane) + [background ∧ levels[0] is the root]
· area(levels[0].stages[0].plane)`: `work_units = stage_work + pixels +
fill`. It moves ONLY for a byte document with a staged level (the unstaged
branch's `total` keeps `rows · count` when `plan.levels` is empty). `run.done`
never exceeds `run.total`: every reported fill is in `fill`, and every
guide's `end ≤ 1` because its work is in `stage_work`.

**Probes and calibration (tests only).** `core/tests/common/probe.rs` now
holds the whole probe kit: `FREEZE` (5 ms; the factor-8 and (m2) probes cap
it at half the timeline's longest gap), `Probe.issued` (the count read just
before `rz_job_control_cancel`, printed by `checkpoints_line`), `Timeline` /
`timeline` (an uncancelled run's checkpoint count at every observed change,
entry through exit; `gap_at(count)`, `longest()`), the report-log trio
(`logged`, `assert_interval`, `assert_pin`, moved verbatim) and
`assert_ramp` (the scan from the first value above `start + 1e-6` through
the first at or above `end − 1e-6`, inclusive — non-empty, non-decreasing,
inside `[start − 1e-6, end + 1e-6]`, at least one value strictly inside).
The preparation suite deleted its copies (`at_the_mask_blur` →
`at_the_blur`, `inside_the_mask_blur` → `inside_the_blur`), the
materialization suite its trio and `FREEZE`, the style suite its `FREEZE`,
the primitive suite its shadowing `inside_the_chain`. The factor-8 mask-blur
probe's `W·c` is the timeline's gap at `p.issued` (timeline from
`at_the_blur` to the first root report, recorded before the probe), freeze
`min(FREEZE, longest / 2)`, `resample()` deleted; (m2)'s `c_copy` is the
timeline's gap at `p.issued` (timeline from `past_the_fill(W, H)` to
`at_the_blur` — the styled group's own drop-shadow blur, after the
materialization), freeze `min(FREEZE, longest / 2)`, `c_copy()` deleted; both
lines print `longest()` beside `issued` as identification, never asserted.
The guide guard `the_byte_guide_blur_leaks_no_report_after_stage_0` is now
`the_byte_guide_blur_reports_inside_its_range` (`assert_ramp(close, guide)`
+ `assert_pin`); `guide_points` adds the one-plane fill (`close = 3A /
work`); the guide probe's entry, `k` and verdict are unchanged (the cancel
lands in the flat grid pass) and its calibration window ends at the guide
blur's first report (`observed(&[before_close, close])`). The five byte
guide tables gain a `"the stage-0 fill"` row (opens at `4096 / work`,
closes at stage 0's open, `f0 − 4096` samples, `f0 = A·(1 + [D5])`) and
every other byte entry/exit gains `f0`; `regional_byte_guide.rs`'s byte test
moved to its u16 twin's shape (`stage_0 = A / work`, `final_stage = (3A +
gw + w) / work`).

## 13A-5b-iii-b-1 contract deltas

13A-5b-iii-b-1 extends the 13A-5b-iii-a contract; everything above not
restated here holds. Tests, the `Makefile` and docs only: no `core/src`,
FFI, header, Swift, MCP, plan-JSON field, `.rz`, tool-table or 11E change;
`regional.rs`'s CANCELLATION MODEL paragraph is untouched; full-frame and
regional output are unchanged by construction.

**The partition.** The four regional cancel suites'
(`regional_byte_preparation_cancel`, `regional_group_materialization_cancel`,
`regional_byte_primitive_cancel`, `regional_style_preparation_cancel`)
default entries assert only load-independent facts: exactness, a cancel
returning `operation cancelled` with no output and zero scratch, nothing
partial cached (the four cache-integrity tests, count-triggered), and the
report-log INTERVAL / PIN guards. Five tests carry
`#[ignore = "make cancel-bench: a wall-clock verdict (13A-5b-iii-b-1)"]`:
`regional_byte_preparation_cancel::the_byte_guide_blur_reports_inside_its_range`
(the RAMP) and `::a_concurrently_cancelled_preparation_blur_returns_within_the_model`,
`regional_group_materialization_cancel::a_concurrently_cancelled_materialization_returns_within_the_model`,
`regional_byte_primitive_cancel::a_cancel_inside_the_primitive_rows_returns_within_the_model`,
`regional_style_preparation_cancel::a_concurrently_cancelled_style_plane_render_returns_within_the_model`.
`make cancel-bench` runs exactly them: `cd core && cargo test --release
--no-fail-fast --test <the four> -- --ignored --test-threads=1 --nocapture`.

**Pre-cancelled controls.** The four `a_pre_cancelled_control_returns_at_once_with_no_output`
tests keep "cancelled, null output, zero scratch" and PRINT their return
time ("13A-5b-iii-b-1 pre-cancelled control returned in … ms (no bound
asserted)"; the style suite's line keeps its `{what}: ` prefix) instead of
asserting `took < 100 ms`.

**The style suite's cache test.** `a_cancel_inside_the_plane_render_caches_no_partial_plane`
cancels with `cancelled_after_checkpoints(&at_the_blur, 0, &inside_the_blur,
whole(&a))` at every size — no freeze, no calibration run; `byte_probe` /
`typed_probe` serve the bench test only.

**`assert_ramp`.** Signature unchanged; clause order (1) `first` = the first
value above `start + 1e-6`; (2) before any scan, `v = log[first]`: `v > end
+ 1e-6` panics naming a logger stall ("already past the end and its
advance"), `v ≥ end − 1e-6` panics naming both a logger stalled to the
advance and a stage reporting nothing inside its range (`checkpoints_only`)
and asks for a serial re-run; (3) `last` and the scan; (4) non-decreasing;
(5) bounds `[start − 1e-6, end + 1e-6]`. The trailing "one value strictly
inside" clause is deleted (subsumed by (2)).

**The fold.** The primitive suite deleted its local `logged` /
`assert_interval` / `assert_pin`, its `progress` and `use std::thread;`
(their only users); the style suite deleted its local trio and keeps
`progress` / `use std::thread;` (`observed` uses both). Both now use
`common::probe`'s (already glob-imported; the local copies shadowed it).

## 13A-5b-iii-b-2 contract deltas

13A-5b-iii-b-2 extends the 13A-5b-iii-b-1 contract; everything above not
restated here holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`,
tool-table or 11E change; full-frame and regional output are unchanged by
construction (the reference chain's bodies are untouched), and the byte plan
and render admit exactly the documents they admitted before (every new work
term is infallible and saturating). `regional.rs`'s CANCELLATION MODEL
paragraph and `core/CLAUDE.md`'s `regional.rs` entry carry the model below.

**The shared run.** `doc_regional_progress.rs` (new) holds `Run` (moved out
of `doc_regional_guide`, now `pub(crate)`) with `range(job, g)` =
`job.range(done/total, (done + g)/total)` and `advance(job, g)` = `done += g`
then `job.report(done/total, 1)` — stage 1, "preparing" — and `PrepWork {
mask, style, materialization }`, one node's counted preparation.
`render_region_with_group_budget` reports against `total = work_units` in
BOTH branches (the unstaged branch used `rows·count` without staged levels).

**The unstaged fill.** Each 4096-sample chunk of the accumulator fill polls
`every` once and then advances `run` by its samples (one more checkpoint:
`report` checks), so the fill ramps `0 → pixels / work` at stage 1.

**The plan's preparation terms.** `work_units = stage_work + pixels + fill +
mask_work + style_work + materialization_work` (the typed
`precision_regional.rs` twin), saturating. The walk (`node()`) computes per
node: `mask_w` — an enabled feathered mask, `MaskProperties::work_units(raw
dims)` = 0 unfeathered or past `padded_dimensions`, else
`plane_blur::float_work(pw, ph, σ)` (new: `float_plan`'s work figure alone,
infallible, saturating, no `pixel_count` refusal; `float_plan` untouched) plus
`2·pw·ph` at outside coverage 255 — the typed `PrecisionMask::plan` figure
without its cap or refusal; `style_w` — a raster whose style renders planes,
`style_scratch(style, pixels dims, false, None)?.1` (the call the walk already
made), or a materialized group whose synthetic renders planes, the same over
the rect's dims (a failure there counts 0: no new refusal); `mat_w` — a
materialized group with a rect, `|rect|·(1 + rows inside) + the subtree's
mask/plane work + its own mask_w`, the rows and the subtree's work taken as
the differences of `needs.len()` and `inside_prep` across its children's walk.
A node inside a materialized subtree adds `mask_w + style_w` to
`inside_prep` and carries no `prep`; a node the render resolves (`collect`
and a `clip`) counts `PrepWork { mask: [not materialized]·mask_w, style:
style_w, materialization: mat_w }`; any other node (a member of an
empty-base unit, a level under an empty isolated extent) carries the figures
but counts nothing and is never resolved — so every counted term is advanced
exactly once and `done` never exceeds `total`.

**The ranges.** `Level::resolve` / `Node::resolve` take the `&mut Run`
(both branches: the unstaged fill → resolve; the staged resolve before
`stage_nested` / `render_staged`). `Node::resolve` dispatches a
`Contribution::Materialized` node to `resolve_materialized` FIRST — before
the mask derivation, whose plane that arm discarded — so the group's own
mask blur runs inside `rendered_group` (the same cached plane,
`effective_mask_rect_with_job` → `derived_with_job`) under the
materialization range; otherwise the mask derives under
`run.range(job, prep.mask)` then advances. `Styled::of(…, run, work)`
renders the planes under `run.range(job, work)` (the `checkpoints_only`
narrowing is gone; a range of 0 work is equivalent) then advances.
`resolve_materialized` hands `rendered_group` `chain.job = run.range(job,
prep.materialization)` un-narrowed — every inner call site keeps its own
`checkpoints_only`, so the chain reports nothing and every inner blur reports
at the range's start — and advances after it; the synthetic's mask stays
`checkpoints_only` (feather 0, no work) and its planes report inside their own
range.

**Stage codes.** Driver reports during preparation — the fill's chunks and
every range's advance — carry stage 1; inner blur reports (`feather`) stay 6;
the loop, the staged fills and the guide advances stay 6; the final
`report(1.0, 5)`. So `stage == 6` is each preparation stage's first inner
report (the 1→6 flip per stage). The typed path reports 6 throughout — a
stated difference.

**The probes (`common::probe`).** `at_the_blur` is `stage == 6` (the first
inner blur report); `inside_the_blur(end)` is stage 6 at or below `end`;
`inside_the_chain(start)` is stage 1 at `start`; `inside_the_inner_blur(start)`
(new) is stage 6 at `start`; `past_the_fill(w, h)` is `2·⌈w·h/4096⌉`
checkpoints (the fill's two per chunk); `loop_start(plan)` (new) is `(work −
output pixels·rows) / work` for an unstaged plan.

**The re-basing.** Every entry that waited for progress above 0 waits above
its own document's `loop_start` (`+ 1e-6`). The preparation suite: `b0 =
pixels / work`, `b1 = 1 − 2·pixels / work`; the verdicts
`inside_the_blur(b1)`; the factor-8 timeline exits above `b1`; the guard
`assert_interval(b1, b1 + 2·W / work)` + PIN; new default
`the_plan_counts_the_mask_blur` (`work − 3·pixels = we·he·32 +
(we·he/f²)·2·taps`, restated) and new bench RAMP
`the_mask_blur_reports_inside_its_range` (`[b0, b1]`, feather 6 and 40). The
style suite (byte): `s1 = 1 − 2·pixels / work`; `byte_window` above each
run's `L`; the probe window exits above `s1`; verdicts `inside_the_blur(s1)`;
guard `assert_interval(s1, s1 + 2·W / work)` + PIN; new bench RAMP
`the_byte_plane_render_reports_inside_its_range` (`[pixels / work, s1]`,
drop shadow 4); u16 untouched. The materialization suite: `m0 = pixels /
work`; (g) / (g′) verdicts `inside_the_chain(m0)`; (s) / (c)
`inside_the_inner_blur(m0)` ((h)'s group mask would report the same way);
(m2) exits `at_the_blur` unchanged; `first_report` above each document's `L`;
guard on `group_doc(N)` `assert_interval(m0, m1)`, `m1 = pixels·(2 + N) /
work`, and `assert_interval(s1, s1 + (N + 2)·W / work)`, `s1 = 1 − (N +
2)·pixels / work`, + PIN. The primitive suite: `inside_the_chain(pixels /
work)`; the `stage == 6` exit unchanged; guard `assert_interval(L, L + 6·W /
work)` + PIN. The six byte concurrent-cancel entries (`regional_document`,
`regional_masks_clips`, `regional_groups`, `regional_byte_effects`,
`regional_byte_styled_groups`, `regional_byte_knockout`) wait above their
`L`. Pins: `regional_byte_effects` `+ P·(2 + 8·2)`,
`regional_byte_styled_groups` `+ 300·(1 + 4) + 750·(2 + 8·2)`,
`regional_masks_clips` `+ plane·32 + plane·2·13`,
`regional_byte_pass_through_guide` (found by the grep) `+ (mw + 14)·(mh +
14)·58` for its two feather-6 gates; `regional_byte_styled_clip_guide`'s
`assert_root_plan` adds `prep = work(bare) − pixels·(bare rows + 1)` and
`measure_byte` shifts every probe by `prep / work` (layers 1 and 3 hidden).

## 13A-5b-iii-b-3 contract deltas

13A-5b-iii-b-3 extends the 13A-5b-iii-b-2 contract; everything above not
restated here holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`,
tool-table or 11E change; full-frame and regional output are unchanged by
construction (no kernel, driver or plan changes — the one library change is
`job_control.rs`, whose new state no kernel or FFI export reads or arms).

**The seam.** `RzJobControl` (still `#[derive(Default)]`, still constructed
only by `rz_job_control_create`) gains a TEST-ONLY, `#[doc(hidden)]`, non-FFI
checkpoint seam: `park_at(count)` arms a park (0, the default, never parks; a
count is the checkpoint's POST-increment count, so ≥ 1) — the checkpoint
whose count equals it parks BEFORE its cancellation test until `release()`;
`parked()`; `wait_parked(timeout)`; `release()` (idempotent; a release BEFORE
the park makes the park fall through, `parked()` still turning true);
`record_reports()` / `reports()`, the worker's own log of every
`RzJobControl::report` call as `Report { checkpoints, progress, stage }` —
the count when the report landed (a `JobContext::report`'s own `check()`
counts BEFORE it), the progress after the report's `fetch_max`, the stage.
`begin()`'s `report(0.0, 1)` and `ffi_jobs.rs`'s closing `report(1.0, 5)`
are recorded like every other report. Every seam lock tolerates poisoning
(`PoisonError::into_inner`), so a poisoned park still releases. Hot-path
cost: one relaxed load + compare per checkpoint (the lock is taken only when
the count matches) and one relaxed load per report (the log's lock only when
recording).

**The wake rule.** `wait_parked` waits on the park's `Condvar` while
`!(parked || released)` and answers `parked`: it wakes on the park, on a
release, or at its timeout, and never spins. A worker that returns without
reaching the count releases on return (`cancelled_parked_at`'s worker
closure), so a never-reached count wakes the waiter at once with `false`.

**The entry-count rule.** The default count-triggered cancel tests take their
ENTRY COUNT `n0` from an uncancelled run of a FRESH twin fixture's own report
log (`common::probe::entry_count` over `reports`: the `checkpoints` of the
first report satisfying the entry — `at_the_blur`, the first stage-6 report)
and cancel with the worker PARKED at `n0 + 1 + k`
(`common::probe::cancelled_parked_at`). `n0 + 1` is the first checkpoint
AFTER the entry report: the report's own `check()` counted `n0` before the
report landed, so parking AT `n0` would freeze the pre-report state. The
verdict is read from the PARKED state (progress, stage, and `checkpoints ==
count` exactly), then the test thread cancels and releases, and the render
must return "operation cancelled", no image and no scratch. `k`: the byte
preparation suite 0 (feather 6 and 40, `inside_the_blur(b1)`), the style
suite's byte cache test 0 at every size (`inside_the_blur(s1)`), the
materialization suite's (s) 0 and (c) `⌈pw·ph/4096⌉/2`
(`inside_the_inner_blur(m0)`). All render kernels are single-threaded
(`thread::spawn` / `scope` appear only in `agent.rs` / `assistant.rs`), so a
fresh fixture's checkpoint and report sequence is identical run to run and
the calibration log transfers to the cancelled run; the twin is FRESH because
the mask/style caches would skip the stage on a second render of the same
document. A count past the render's last checkpoint fails at once — "the
worker did not park at N: it returned first (the count lies past the
render's last checkpoint) or 240 s elapsed" — a deterministic calibration
verdict, never a load verdict.

**The `Released` guard.** `common::probe::Released(&control)` cancels, then
releases, on drop. `cancelled_parked_at` and every seam contract test that
can assert while a worker is parked hold one inside the `thread::scope`
closure, so a panic unwinds through it BEFORE the scope's implicit join and
the join returns at the worker's next checkpoint (or its fall-through park)
instead of hanging. On the normal path it is a no-op in effect (the worker
has already returned). A parked worker holds a live handle, so no control or
document is freed while parked.

**Which suites.** Three suites have default count-triggered tests, now all
parked: `regional_byte_preparation_cancel` (the mask blur cache test),
`regional_style_preparation_cancel` (the byte plane cache test),
`regional_group_materialization_cancel` ((s) the inner style render, (c) a
child mask blur). `regional_byte_primitive_cancel` has none (its only
count-triggered test is the bench's). The bench tests (`#[ignore]`, `make
cancel-bench`) keep `probe` / `Trigger` / `Freeze` / `timeline` / `window`,
their bounds and tables unchanged; the INTERVAL / PIN guards keep `logged`.
The printed cache-integrity lines are `parked_line`'s ("parked at checkpoint
N (progress p, stage s), released L ms → return"); `latency` (release to the
worker's return) is printed only — the default `make test` gains no
wall-clock assertion (13A-5b-iii-b-1's policy; the seam test (f)'s "woke
before the deadline" is the existing 240 s deadline shape, not a bound).

**The seam's own contract** (`core/tests/job_control_park.rs`, 7 tests): (a)
a worker parks exactly at `park_at(N)` (`checkpoints() == N`, not finished),
and a cancel + release returns "operation cancelled" at N; (b) a release
before the count makes the park fall through (`parked()` true, the loop
completes); (c) a default / `park_at(0)` control never parks and
`wait_parked(50 ms)` is false; (d) a 64×64 two-raster `rz_doc_render_region`
under `record_reports` logs `begin`'s `(1, 0.0, 1)` first, non-decreasing
counts and progress, and the close `(1.0, 5)` last; (e) a parked-then-
cancelled `rz_doc_render_region` returns NULL with "operation cancelled" and
no scratch; (f) a worker short of the count that releases on return wakes
the waiter with `false` before the deadline; (g) dropping a `Released` guard
cancels and releases a parked worker, which returns "operation cancelled" at
N.

## 13A-5b-iii-c-1 contract deltas

13A-5b-iii-c-1 extends the 13A-5b-iii-b-3 contract; everything above not
restated here holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`,
tool-table or 11E change; plan `work_units`, scratch and refusals are
unchanged; full-frame and regional OUTPUT are unchanged at 8, 16 and float
(no pixel statement moved: `build`'s `weighted_plane` / `coverage_plane` /
`plane` arithmetic is untouched, and the exactness suites — the byte and
typed guide files, the five guide files, the style suite's two "match the
full frame" tests — pass unchanged). Only PROGRESS reports move.

**`GuideWork` (the guide's work terms, one home).** `adjust_precision_plan.rs`
gains `GuideWork { pixels, cells, taps }` (`of(w, h, radius)`: infallible and
saturating — `sigma_for_size` → `downsample_factor` → `cells = ⌈w/f⌉·⌈h/f⌉`,
each ≥ 1 → the grid sigma, reduced above factor 1 → `taps`, 0 at a grid
sigma ≤ 0, else `nearest_odd(feather_radius·2 + 1).max(3)`; `pixels = w·h`);
`total() = pixels + cells·(2 + 4·taps)`; `blur_ranges()`, of `t = total()`:
`[((p + c)/t, (p + c + 2·c·taps)/t), ((p + c + 2·c·taps)/t, (t − c)/t)]`.
`guide()` keeps its signature, refusals (pixel count, then radius) and both
figures, now read through `GuideWork` (`cells·24 + taps·4 + 4096`,
`total()`); `build` reads the ranges from it and never restates the formula.

**The guide's sub-ranges.** `adjust_guide::build` runs its first grid blur
under `job.range(r[0])` and its second under `job.range(r[1])`: the grid pass
and per-cell means poll FLAT at the caller's range start (`[0, (p + c)/t]`),
blur 1 ramps its sub-range (its `feather` rows; the u8 blur at the reduced
sigma through `plane_blur`, the f32 one through `gaussian_plane::feather`),
blur 2 ramps the next, the combine (`[(t − c)/t, 1]`) is flat, and the
caller's advance lands at the range's end. `JobContext::range` nests, so
every caller gets the split shape: the byte staged driver
(`doc_regional_guide.rs`, `run.range(done/total, (done + g)/total)`), the
typed staged driver (`precision_regional_guide.rs`,
`progress.job.range(start, end)`), the full-frame typed render
(`precision_render.rs`, `job.range(start, end)`) and the destructive typed
adjustment (`precision_adjustment.rs`, `job.range(0.05, 0.4)`); the byte
full-frame `tone_plane` passes `NONE` and reports nothing. `taps == 0` gives
empty sub-ranges (the f32 blur returns at sigma ≤ 0; a u8 `feather` under an
empty range reports its start, a no-op under `fetch_max`). The fractions are
of the plan's OWN guide work, so they are the plan's terms, not a timing:
the byte guided fixture's `R_0` (3000×2000, radius 30: factor 8, 375×250
cells, 9 taps; `t` = 6,000,000 + 93,750·38 = 9,562,500, the plan's `g`)
puts blur 1 at 0.6373–0.8137 of its range and blur 2 at 0.8137–0.9902.

**The equal per-effect split.** `style_render::render_effects_with_job`
renders effect `i` of the `n` enabled effects under `job.range(i/n,
(i + 1)/n)`. EQUAL is the declared choice: `style_scratch` charges a uniform
`8·p` per enabled effect, so the plan has no finer term to split by. Under a
`checkpoints_only` job (the byte `resolve_materialized`'s inner sites, the
typed `composite_styled_into`) a sub-range of a zero-width range is
zero-width — no shape change there. The byte `Styled::of` path (13A-5b-iii-b-2)
gets the split for multi-effect styles; the single-effect fixtures (drop
shadow 4 / 40, inner bevel 4) keep their shape. Inside ONE effect its first
blur ramps the effect's sub-range and its later blurs re-ramp under
`fetch_max` and hold its end (the stated residual; open items).

**The typed style range.** `precision_regional_tree::prepare_style` computes
`work` (`style_scratch(..).1`) BEFORE the render and runs
`render_effects_with_job` under `progress.job.range(completed/total,
(completed + work)/total)` (no `checkpoints_only`), then `advance(work)` as
before. The typed regional style stage therefore ramps `pre → target` (a
lone drop shadow: the blur's `feather` rows, whose last row reports the
range's end `target`; at factor 8 the reduced plane's rows, the resamples
flat; the shift/multiply passes after the blur flat) and the advance
re-reports `target`. Every typed report still carries stage 6 (the typed
stage-code difference, unchanged). `composite_styled_into`'s own render
stays `checkpoints_only` (13A-5b-iii-c-2's).

**Test deltas.** (1) `regional_byte_preparation_cancel.rs`:
`guide_sub_ranges()` restates `(b1, b2, b3) = close + (guide − close)·{(p +
c)/t, (p + c + 2·c·taps)/t, (t − c)/t}` from `R_0`'s dimensions at radius 30
with the file's own `factor` / `kernel_radius` / `nearest_odd`, asserting
`t` equal to the planner's `guide_work`; bench
`the_byte_guide_blurs_each_report_inside_their_sub_ranges` = `assert_ramp(b1,
b2)`, `assert_ramp(b2, b3)`, `assert_pin`. `the_byte_guide_blur_reports_
inside_its_range` and the guide probe (it lands in the flat grid pass, at
`close`) are unchanged. (2) `regional_style_preparation_cancel.rs`:
`typed_points` returns `(pre, target, work)` and asserts the plan's
`cancellation_checkpoint_pixels` is the test's `CHUNK` (4096,
`render_request::CHECKPOINT_PIXELS`); the default guard is renamed
`the_typed_plane_render_leaks_no_report_into_the_root_range` and re-based to
`assert_interval(target, target + 2·CHUNK/work)` + PIN at every size (the
loop's first chunk advances `CHUNK·rows`, two layers); new bench
`the_typed_plane_render_reports_inside_its_range` = `assert_ramp(pre,
target)` + PIN (drop shadow 4); `typed_window`'s BOTH exits are
`observed(&[target + 1e-6], ..)` with each run's own `target` — the loop's
first chunk, mirroring `byte_window`'s `loop_start + 1e-6`, so `T_S − T_0`
covers the whole plane render (blur, post-blur passes, advance) at both
sizes; `typed_probe`'s entry is the first value above `pre` (`p > pre +
1e-6`, the blur's first row), its window's exit the blur's last row (`p ≥
target − 1e-6` — the probe's window only, never the timing exit), `k` half
that window (size 4) or the 5 ms freeze (size 40), and its verdict `pre +
1e-6 < p ≤ target + 1e-6` — INCLUSIVE of the advance's own value, the
verdict's stated tolerance, because typed reports carry no stage flip that
could tell the blur's last row from the advance. Bench bounds and the
`interval()` costs are unchanged. (3) NEW `regional_typed_preparation_cancel.rs`:
`guided16()`, the byte `guided()`'s twin at u16 (an opaque seeded backdrop,
a Multiply raster, the radius-30 `precision_v1` guide, a SoftLight raster;
builders copied from `regional_precision_guide.rs`); `guide_points16` =
`(close, guide, next)`: `3A/work`, `(3A + g)/work`, and `guide + 4096/work`,
the output zero fill's first chunk (`render_staged`'s `Last::Output` →
`zero_plane`); default `the_typed_guide_leaks_no_report_after_its_advance`:
(i) on the worker's OWN report log (`reports`, under `record_reports`) the
first report above `guide + 1e-6` is within 1e-6 of `next` — the
deterministic equality; (ii) on the spinning `logged` trace only the
one-sided `assert_interval(guide, next)` + `assert_pin`; bench
`the_typed_guide_blurs_each_report_inside_their_sub_ranges` (two RAMPs +
PIN over `guide_sub_ranges16`). `make cancel-bench` runs it as its fifth
binary.

**The carried nits.** `common::probe::cancelled_parked_at`'s worker holds a
private `ReleaseOnDrop(&RzJobControl)` across `render(token)`, so a
panicking render releases on unwind and the waiter wakes at once instead of
after 240 s (the explicit release on the normal path stays). The
`regional.rs` module doc's MATERIALIZATION paragraph is refilled to ~76–78
columns with no wording change (and the CANCELLATION MODEL lines this run
touched), the two clauses above restated there.

## 13A-5b-iii-c-2 contract deltas

13A-5b-iii-c-2 extends the 13A-5b-iii-c-1 contract; everything above not
restated here holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`,
tool-table or 11E change; plan `work_units`, scratch and refusals are
unchanged; full-frame and regional OUTPUT are unchanged at 8, 16 and float
(only a `JobContext` argument moved at each site — every pixel statement is
untouched; the exactness suites pass unchanged, and the new suite's
`the_polled_materialized_composite_matches_the_full_frame_at_u16` compares
the full frame under a live control with the uncontrolled one and four
regional windows — the canvas, an interior window, a top-right edge window
and a 1×1 at the bottom-left — with the projection's crop). Typed styles do
not exist at float (the float operator table refuses them), so the typed
statements are u16 ones. Only PROGRESS reports and CANCELLATION polls move.

**The live job at the shared primitives.** The seven typed call sites of the
shared primitives pass the composite's own `job` instead of
`JobContext::NONE`: in `precision_style_render::composite_direct_into`,
`BackdropSnapshot::of`, the Below and the Interior `composite_contribution`
loops, the clip base's per-member `clamp_alpha_to_shape` and the final
`composite_buffer_into`; in `precision_style_knockout::composite_interior_unit_into`,
the grouped-interior `composite_contribution` and the per-member
`clamp_alpha_to_shape`. Each polls once per ROW (13A-5b-ii-b's byte form)
and reports nothing, so a cancel inside a typed styled composite after its
planes now returns within `T_f + c_row` (the "inside the style/knockout
primitives" row form) instead of one unit per primitive.

**The composite's plane range.** `precision_style_render::composite_styled_into`
renders its planes under the job it is handed (the `checkpoints_only` is
gone), and `precision_render::render_styled` hands it `planes_job =
progress.job.range(completed/total, (completed + work)/total)`, `work` the
layer's `style_scratch(style, plane, false, None).1` — `prepare_style`'s
rule, computed before the member closure borrows `progress`. A failing
charge (`p > MAX_PIXELS`) leaves the range empty (`map_or(start, ..)`) and
fails at the unchanged `style_scratch` call after the composite, where it
always did, so no error moves. The composite's shape: the planes ramp
`completed → completed + work` (equal per-effect sub-ranges; a lone drop
shadow's blur rows, the last reporting the end), then the Below
contributions, the pixel pass and the Interior contributions poll only, the
base advance re-reports the end whenever `base_work < work` (held under
`fetch_max`), and the style advance lands at `completed + base_work + work`.
This is the full-frame typed render's own path (`render_layers`), so the
FULL-FRAME typed render under a live control ramps there too — progress
only; and it is the typed materialization's (`prepare_style` → `render_layers`
over a styled group's children). A styled clip base's members render after
the planes from the base's own `completed`, so their reports hold under the
planes' end until they pass it (a stated residual; open items). The typed
PSD bake keeps its old shape: `bake_style16` hands `job.range(0.2,
0.9).checkpoints_only()` (the narrowing moved to its call site — 13A-5a's
"`checkpoints_only` at the CALL SITE" rule), so its planes still report
nothing.

**The measured interval (NEW `regional_typed_styled_composite_cancel.rs`, in
`make cancel-bench`).** Fixture `materialized16`: 3000×2000 u16 sRGB, an
opaque Background, an isolated Normal group with a Multiply colour overlay
(materialized) over ONE canvas-sized hole-alpha child with a drop shadow 4
at distance 0 (factor 1). With `A = W·H`, `S = 10·pw·ph` (the child's padded
plane written out independently, `pad = ⌈kernel_radius(2)⌉ + 1 + 2`) and the
plan's `work`: `s = 2A/work` (the acc fill and the group buffer's zero
fill), `e = (2A + S)/work` (the planes' end) and `adv = (3A + S)/work` (the
child's style advance). DECLARED before measuring: a cancel inside the
composite after its planes returns within `T_f + c_row`, `c_row = t_comp /
n_comp` over the uncancelled window from the planes' last report (`p ≥ e −
1e-6`) to the style advance (`p > e + 1e-6`); the probe enters at that last
report, cancels `k = n_comp / 4` checkpoints later (inside the Below
contribution's H rows), verdict `|p − e| ≤ 1e-6`. Default (load-independent,
the worker's OWN report log): (i) at least H distinct values strictly inside
`(s, e)` (the blur's two passes report per plane row; under the old
`checkpoints_only` the only value there was the base advance's `3A/work` —
the test fails with "1 distinct reports" on the old sources); (ii) the first
report above `e + 1e-6` is `adv` and the one before it (the base advance)
holds at `e`; (iii) at least 2·H checkpoints between the planes' last report
and the base advance (the Below contribution's H rows and the pixel pass's
H rows — with the Below `composite_contribution` alone reverted to `NONE`
it fails at 2004); plus a parked cancel at `H/2` checkpoints past the planes'
last report (`cancelled_parked_at`, verdict `|p − e| ≤ 1e-6`, release-to-
return printed only). Bench: `the_materialized_composite_planes_report_inside_their_range`
(`assert_ramp(s, e)` + PIN over the spinning trace) and
`a_concurrently_cancelled_materialized_composite_returns_within_the_model`
(the PASS/MISS line). `make cancel-bench` runs it as its SIXTH binary.

**The carried nits.** `regional_style_preparation_cancel.rs`'s module doc is
refilled at lines 1–9 and 56–68 to the file's ~77-column width (no wording
change); `regional.rs`'s orphan `` //! `prepare_style` `` line is joined with
the next as part of the style paragraph's rewrite.

## 13A-5c-1 contract deltas

13A-5c-1 extends the 13A-5b-iii-c-2 contract; everything above not
restated here holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`,
tool-table, 11E or README change; the plan's JSON schema, `contract_version`
1, `work_units` and every refusal (order and sentence) are unchanged;
full-frame and regional OUTPUT are unchanged at 8, 16 and float. The typed
path is untouched (decision below).

**The description preparation's scratch.** `adjust_calibrated::context_scratch`
(the byte cutout's `refinement_context_scratch` body, moved with its
256-bytes-per-input-byte rationale; the cutout keeps its own sentence) is
`Some(0)` under a matrix/TRC profile — `validate_context` parses nothing
there — else the largest non-group `meta` length × 256, `None` past
`MAX_RZDC_META_LEN`. The byte plan's `scratch_heap_bytes` adds it as its last
term. It is profile-gated exactly as the cutout's rule, and no regional
fixture before this run uses an unmodelled profile, so every existing byte
plan pin (`scratch_heap_bytes` in the `regional_*` suites) is unchanged.
The walk's own `from_meta` transient under a MODELLED profile stays
uncharged (memoized above 64 KiB; open item, 13A-5c-3).

**The admission under the render's job.** `regional_admission<'a>(&'a self,
.., job: JobContext<'a>)` (one named lifetime: `Walk<'a>` unifies `doc`,
`env` and the returned `Level<'a>`): the plan passes `JobContext::NONE`, the
render its job. In order: `context_scratch` (refusing "layer metadata
exceeds the native slot limit" past the slot cap — unreachable through the
FFI, whose `set_meta` caps at 16 MiB), `job.reserve(context)`,
`validate_context_with_job(&self.layers, &self.profile, job)` (a check
before and after each layer's parse), `self.transfer()`, then the walk with
`env: CompositeEnv { job, ..self.composite_env(None, None) }`, whose
`Walk::node` polls `self.env.job.check()` first. The render's later
`job.reserve(plan.scratch_bytes)` REPLACES the context reservation
(`RzJobControl::reserve` stores, it does not add). The parse precedes the
plan's first report and reports nothing (ONE unit per parse; no progress
change).

**The walk's `operator`.** An adjustment row's plan `operator` is
`Adjustment::op_name` (22 arms, the inverse of `from_op`'s string match; the
`Calibrated` arm delegates to the new `Calibrated::op_name`, the private
`op` field staying private), taken before the value moves into
`Contribution::Adjustment` — the whole-meta `serde_json::Value` re-parse the
walk ran per adjustment row is gone. The pinned strings are unchanged
(`regional_document.rs`'s op loop; the new suite's `levels` rows).

**The materialization's re-validation.** `doc_group::materialized` and
`composite_level_into` call `validate_context_with_job(layers, env.profile,
env.job)`: `env.job` is `NONE` for every full-frame composite and live only
from `resolve_materialized` (13A-5b-ii-a's rule), so the full frame is
byte-identical by construction and only a regional materialization polls it.
Every other `validate_context` caller (`doc.rs:773`, `doc.rs:1166`, …) is
untouched. The memo is transparent and stores only COMPLETE parses
(`adjust_lut`'s module doc): a cancelled render leaves no partial state.

**The measured interval (NEW `regional_context_preparation_cancel.rs`, in
`make cancel-bench` as its SEVENTH binary).** Fixture: 256×192 under an
UNMODELLED RGB profile (sRGB's header with a zero tag count), an opaque
Background and three transparent canvas-sized rasters carrying a Levels
description padded to 4 MiB by a top-level numeric `pad` array headed by a
per-fixture NONCE (the memo would otherwise skip every parse after the
first); the materialized twin adds an isolated Normal group with a Multiply
colour overlay around one more padded Levels layer. Checkpoint model, from
the code: `begin` 1, the render's `check` 2, the context `reserve` 3,
`validate_context_with_job`'s leading check 4, then two per layer of ANY
kind (layer 1's parse between 7 and 8). DECLARED before measuring: stage
"description preparation", `W_s` = ONE meta parse (serde cannot be polled
inside a parse), bound `T_f + c_parse`, `c_parse` the worker's own
uncancelled checkpoint-timeline gap at the count the cancel was issued at
(`Timeline::gap_at`, 13A-5b-iii-a's `c_copy` form). Default: the regional
render equals `rz_doc_flattened` cropped / sampled (the canvas, an interior
window, a top-right edge window, a 1×1 at the bottom-left, a divisor-2
preview); the plan charges `pixels·16 + output_bytes + 4 MiB·256` on the
fixture and `pixels·16 + output_bytes` on its sRGB twin, `work_units`
`pixels·5` in both, `operator` `levels` on rows 1–3; the checkpoint count on
the fixture less its sRGB twin is `2·L` flat and `4·L` materialized; a cancel
PARKED at checkpoint 8 returns "operation cancelled", no image, no scratch,
at progress 0 stage 1. Bench: the timeline at 7 → 8, then a freeze probe
(`min(5 ms, c_parse / 2)`) from count 7, returning at checkpoint 8 (the
PASS/MISS line).

**The carried nit.** `regional_typed_styled_composite_cancel.rs`'s module doc
is refilled to ≤77 columns at its declared-bound and load-robustness
paragraphs (no wording change).

## 13A-5c-2a contract deltas

13A-5c-2a extends the 13A-5c-1 contract; everything above not restated here
holds. No FFI signature, Swift, MCP, `.rz` or tool-table change; the header
changes are comment-only (`rz_doc_retouch_source`: per tile, and per row
inside the full-frame fallback; the flattened-promotion block: legacy
groups/styles/spatial rendering cancel per row inside that render, and
`scratch_bytes` includes the description-context term). Every plate's
OUTPUT is unchanged: the tiled path equals `rz_doc_flattened` byte for byte
for every newly admitted form, and the fallback is the same projection under
a live job (every `_with_job` twin is the same bytes).

**The fallback under the job.** `RzDocument::flattened_cut_with_job(group_pixels,
cut, job)` is `flattened_cut`'s body with `validate_context_with_job(.., job)`,
`CompositeEnv { cut, job, .. }` and `encode_projection_with_job(.., job)`;
`flattened_cut` is its `JobContext::NONE` wrapper, so its three callers
(`doc.rs`'s `flattened_with_group_budget`, `doc_color_policy.rs`,
`precision_scoped_source.rs`) and every full-frame projection are unchanged.
`inpaint_source::sampling_source` runs the fallback under
`control.checkpoints_only()`: the context parse, the compositor chain
(13A-5b-ii-a/b's per-row polls, every child mask blur through
`active_mask_with_job`, the style plane renders) and the encode all poll the
sampling job, and a cancelled mask blur caches nothing. The plate still
REPORTS no progress (a stage-6 report inside a mask blur lands at the
range's start, progress 0 for `rz_doc_retouch_source`); a per-tile /
per-row report is an open item (13B).

**The widened tiled admission.** One private `tile_safe(layer, meta_admits)`
— `kind == Raster && depth == 0 && style.is_none() &&
meta.is_none_or(meta_admits)`, with NO mask clause — and two predicates over
it: the PLAN-time `pointwise(doc)` (`promote_flattened_plan`) admits no
metadata at all, so it parses nothing (the cutout rule "never parse JSON in
the plan") and may answer full-frame where the render tiles, which only
over-reserves; the RENDER-time `pointwise_cut(layers, cut)` admits metadata
iff `Adjustment::from_meta(meta).is_none_or(|a| a.is_pointwise())`, over the
CUT stack as before. So every mask form (feather, density, a mask offset or
size unlike its layer's, outside coverage 255 — the aligned plane is
layer-anchored and cached, so a tile's crop is the full frame's), pointwise
adjustments and opaque metadata (`composite_layer_into` never interprets it)
tile. Groups, styles and guided Shadows/Highlights stay on the fallback: D5's
Background snapshot is (0,0)-anchored, `rendered_group` clips a group's
extent to the accumulator (a styled group's planes differ at a tile edge),
and `guide_with_job` builds from the accumulator it is handed. Tiling groups
and styles is 13A-5c-2b.

**The reservation.** On top of the caller's `base`: the tiled path reserves
`tile_scratch_bytes(layers, cut)` — over visible, non-cut, mask-enabled
layers whose mask needs derivation (feather > 0, density ≠ 1, mask offset ≠
layer offset, mask dims ≠ pixel dims), `(feather > 0 ?
mask_properties.scratch_bytes(mask dims) : 0) + pixels.w·h`,
`projection_scratch_bytes`' own per-layer mask term, cut-aware, saturating;
the fallback `projection_scratch_bytes − plate` as before. BOTH add
`context_scratch(&layers, &profile)` (13A-5c-1's term; `Some(0)` under a
modelled profile, so no sRGB reservation moves), refusing "layer metadata
exceeds the native slot limit" past the slot cap (unreachable through FFI
and `.rz`). The tiled path runs `validate_context_with_job(.., control)`
before `transfer()` (`flattened_cut`'s refusal precedence) and composites
under `CompositeEnv { cut, job: control.checkpoints_only(), .. }`; its tile
loop is unchanged.

**The promotion plan covers the render.** `promote_flattened_plan`'s
cooperative arm adds `tile_scratch_bytes(&layers, Cut::NONE)`; BOTH arms add
`context_scratch(..).ok_or(..)?` (string lengths only), so `scratch_bytes =
render.max(12n) + metadata_parse + context + 4·BK` bounds the tiled
reservation (`4n + 4·BK + tile_scratch(cut) + context`, a cut only removing
contributors) and the fallback's (`projection_scratch_bytes + 4·BK +
context`) whichever arm the plan predicted. `json()`'s `render_cancellation`
is `"cooperative"` in both arms (the fallback is cooperative now).

**The declared bounds** (before measuring, `inpaint_source`'s module doc): a
cancel inside the TILE LOOP returns within `T_f + c_tile_row` (one row of one
layer's tile composite, ≤ 1024 samples); a cancel inside the FALLBACK within
`T_f + c_row` (one row of the running chain primitive, `regional.rs`'s
model). Calibration (NEW `regional_sampling_fallback_cancel.rs`, the EIGHTH
`make cancel-bench` binary): `T(n)` the minimum of three uncancelled plates
of a fresh 3000×2000 fixture — `n` canvas-sized rasters inside an isolated
group (fallback) or under aligned, unmodified masks (tiled, no derivation) —
`c_row = (T(6) − T(1)) / (5·H)`, `c_tile_row = (T(6) − T(1)) /
(5·H·⌈W/1024⌉)`; the probe enters at half the uncancelled twin's checkpoint
count and cancels one checkpoint later (`Trigger::After(1)`).

**The new suite's default tests** (canvas 2100×150: three tile columns by
three tile rows, partial edges on both axes): (1) nine tiled forms — a
feathered mask on an offset layer, density 0.4, a mask offset unlike the
layer's, mask dims unlike the layer's, outside coverage 255 with a feather,
a pointwise Levels at opacity 0.7 under a feathered mask, opaque
`{"type":"note"}` metadata, a clip run over a masked base, Dissolve at 0.5 —
each equal to `rz_doc_flattened` AND succeeding under a scratch limit of
`plate + 1 MiB + T + 256 KiB` (`T` the test's own `tile_scratch_bytes`
mirror; the fallback would need `plate + 1 MiB + ≥ 16·W·H`: the route
witness); (2) the fallback (an isolated group with a feathered child mask, a
drop shadow, a guided Shadows/Highlights radius 8) equal to the projection
under a live job, the guided fixture refused under test 1's limit with
`RzJobControl::reserve`'s "working-memory limit" and scratch 0, and Current
and Below / ignore adjustments over a depth-0 unclipped Levels equal to the
projection of the stack without it; (3) cancels PARKED inside the fallback's
child mask blur (the first stage-6 report's count + 1; progress 0, stage 6;
no mask plane cached; an uncancelled re-run equals the projection), inside
the fallback's chain and inside the tile loop (half the uncancelled twin's
checkpoint count, which must be at least `2·H`: an unpolled chain or a
per-tile-only loop totals a handful), each returning "operation cancelled",
no image, scratch 0. `precision_conversion.rs` gains the unmodelled-profile
twin of the tiny-canvas large-metadata test: `plan.scratch_bytes ≥ 64·L +
256·L + 4 MiB`, not cooperative, and `promote_flattened` succeeds under a
token limit of `plan.scratch_bytes` with scratch 0 after.

**The carried nits.** `doc_regional_walk.rs`'s `Walk.env` doc names the
admission's live job, polled once per node;
`regional_context_preparation_cancel.rs`'s two count deltas use
`checked_sub(..).expect("the unmodelled twin polls at least as often")`.

## 13A-5c-2b contract deltas

13A-5c-2b extends the 13A-5c-2a contract; everything above not restated here
holds. No FFI signature, Swift, MCP, `.rz` or tool-table change; the header
changes are comment-only (`rz_doc_retouch_source`: per tile; per tile row
through the regional tree; per row inside the full-frame fallback, now only
guided Shadows/Highlights; the flattened-promotion block: groups/styles
cancel per tile row through the tree, and `scratch_bytes` carries the tile
term below). Every plate's OUTPUT is unchanged: the tree plate equals
`flattened_cut` byte for byte, and every regional render passes `Cut::NONE`,
so `regional_render`, `regional_groups`, the `regional_byte_*` suites and the
`render_region_plan` JSON are byte-identical.

**The byte walk takes a `Cut`.** `regional_admission(request, max_output,
group_pixels, cut, reserved, job)` is `pub(crate)` and returns `Admission {
plan, level, transfer, preparation_scratch }` (`preparation_scratch` = the
walk's mask + style + materialization scratch + the context parse: every
term of `plan.scratch_bytes` but the accumulator, the output and the stages);
it reserves `reserved + context` before `validate_context_with_job`. The
walk's env carries the cut: the unit skip is `!contributes(entry) ||
cut.excludes(unit.idx)` (the reference's `entry.visible &&
!env.cut.excludes(unit.idx)`), a cut clip member is dropped BEFORE
`contributes` (the reference's `resolved_member` marks it invisible, which
both member loops drop), and `visible_members`, `is_isolated`, `clip_run`,
`pass_through_holds` and `level_wants_shallow` keep reading the DOCUMENT's
flags (`Cut`'s rule: the isolation decision must not move). The dry run's
three `visible` guards (`doc_regional_budget.rs`) add `&&
!env.cut.excludes(idx)`, and D5's `background_entry` takes the cut (the
reference passes `env.cut`). `group_extent` / `rendered_group` already read
`env.cut`. The plan and the render pass `Cut::NONE, 0`.

**The tree plate.** `RzDocument::regional_cut_plate(cut, group_pixels, base,
control)` (NEW `doc_regional_plate.rs`): admits the full canvas at divisor 1
(`max_output_bytes` unbounded) with the cut and `base` reserved; refuses a
staged level ("the sampling plate admits no staged level", unreachable
through `plate_arm`); reserves `base + 20·tile + preparation_scratch`
(`tile = tile_pixels(canvas) = min(1024, W)·min(64, H)`, the plate's, the
plan's and the tests' one figure; 16 bytes of f32 accumulator and 4 of RGBA8
encode per tile pixel); resolves the tree ONCE over the canvas clip under
`CompositeEnv { cut, job: control.checkpoints_only(), .. }` and one
`RenderBudget::new(group_pixels)`; then for each ≤ 1024×64 tile polls, fills
a fresh tile accumulator per tile row (a poll at each row's head, then
`ResolvedLevel::sample` per sample with the root's `KnockoutBackdrop::NONE`),
encodes it with `encode_projection_with_job` (per-row polls) and copies it
into the plate; finally polls and drops its reservation to `base`. The plate
reports no progress (13A-5c-2a's decision): every preparation report lands
at 0.

**Three arms.** `inpaint_source::plate_arm(layers, cut, canvas, control)`
REPLACES `pointwise_cut`: one pass over the cut stack, one
`Adjustment::from_meta` per contributing non-group entry with metadata and a
`control.check()` before each parse (carried nit 1). `Tiles` when the canvas
has a zero side or every non-cut entry is `tile_safe` with metadata none or
pointwise (13A-5c-2a's rule, unchanged); else `Tree` when every non-cut,
visible, non-group entry's metadata is none or pointwise (a guide-reading
Shadows/Highlights is the tree's only STAGED operator); else `FullFrame`
(`flattened_cut_with_job`, unchanged). The plan-time `pointwise` and
`tile_scratch_bytes` are unchanged.

**The declared bounds** (before measuring, `inpaint_source`'s module doc):
Tiles `T_f + c_tile_row` (one row of ONE layer's tile composite, unchanged);
Tree `T_f + c_tree_row` — one full tile row of ROOT samples through the
WHOLE tree (sample-major, ≤ 1024 samples), polled at every tile row's head
and per encode row, its preparation under the regional stages' own measured
bounds (mask blur 13A-5a, planes 13A-5b-i, materialization chain
13A-5b-ii-a/b, description parse 13A-5c-1); FullFrame `T_f + c_row`
(unchanged); the predicate pass `T_f + c_parse` (13A-5c-1's model); the
`from_meta` transient stays uncharged (13A-5c-3's row). Calibration (NEW
`regional_sampling_tree_cancel.rs`, the NINTH `make cancel-bench` binary):
`T(n)` the minimum of three uncancelled plates of the fallback suite's
`bench_group(n)` (3000×2000, `n` canvas-sized rasters in an isolated group,
now Tree), `c_tree_row = 1024 · T(6) / (BW·BH)` — `T(6)`'s per-root-sample
cost times one full tile row (its preparation, encode and copy only add
slack); NOT the Tiles arm's `(T(6) − T(1)) / (5·rows)`, one LAYER's marginal
row, which is wrong for a sample-major tree.

**The promotion plan covers the tree.** `promote_flattened_plan`'s
non-cooperative arm is `projection_scratch_bytes + (20·tile − 16·n)⁺`. The
tree reserves `base + 20·tile + preparation` with `base = 4n + 4·BK`, and
`preparation ≤ projection_scratch_bytes − 20n` term by term (masks: the same
`MaskProperties::scratch_bytes` plus plane; styles: `style_scratch(..).0` ≤
128·P, 0 for an effect-less style where the projection still charges 16·P;
materialization `16·peak + 5·pixels + planes` ≤ 56·depth·n), so the arm
covers the tree exactly when it also covers `20·tile` against the
accumulator's 16n — which fails only where `20·tile > 16n` (W < 1280 with
H ≤ 64, or W ≤ 1024 with H < 80): hence the term.

**The test seam.** `#[doc(hidden)] pub fn
RzDocument::retouch_plate_full_frame(idx, mode, ignore_adjustments)`
(`retouch_sampling.rs`, the `render_region_with_group_budget` precedent):
Current → `layer_canvas_image(idx)` iff `raster_layer(idx)`; else
`flattened_cut(MAX_GROUP_BUFFER_PIXELS, cut_of(retouch_cut(..)))` — the
black-box full-frame reference for cut plates.

**The new suite's default tests** (`regional_sampling_tree_cancel.rs`,
canvas 2100×150): (1) eleven tree forms — (a) an isolated Normal group of
two rasters, one under a feathered (6) mask, (a0) the same unfeathered, (b) a
pass-through group at 0.7 under a feathered (3) mask holding a Multiply
raster and a pointwise Levels, (c) isolated ⊂ pass-through ⊂ isolated with a
clip run (raster base, Screen and Levels members), (d) a raster with drop
shadow + outer glow, fill opacity 0.6 and a Blend If, (e) a styled group
(drop shadow 4) straddling x = 1024 and y = 64, (f) knockout (a
Background-qualifying entry 0, a Deep styled raster at depth 0, a Shallow one
inside a pass-through group), (g) a styled group inside a styled group, (h) a
group clip base with two members, one a group, (i) an empty and a hidden
group beside (a), (j) Dissolve 0.5 inside a group — each equal to
`rz_doc_flattened` AND succeeding under the ROUTE WITNESS limit `L =
plan.scratch_heap_bytes − 16·n + 20·tile + 1 MiB + 256 KiB`, which the
fallback cannot meet; (2) for (b), (c), (e), (h) with a Levels inside the
group and one on top, the plates for the group's first child in Current and
Below and All + ignore adjustments equal `retouch_plate_full_frame`; (3)
cancels PARKED under `L` inside (e)'s preparation (the first stage-6 report's
count + 1; progress 0, stage 6; the re-run equals the projection) and inside
(a0)'s tile loop (half the uncancelled count, which must be at least 3·H);
(4) the promotion pin on (a), (e), (t1) a 10×10 effect-less styled raster and
(t2) an isolated group of two 10×10 rasters on 600×40 (`20·tile = 20n >
16n`), and on (t1) `plan.scratch_bytes ≥ projection_scratch_estimate_bytes +
4n`. The four `retouch_sampling.rs` group-cut tests pass unmodified through
the tree.

**The re-based fallback suite.** `regional_sampling_fallback_cancel.rs`: its
group and style fixtures now route Tree (`grouped_and_styled_plates_equal_
the_projection_under_a_live_job`); the parked mask-blur test keeps
`group_masked(6.0)` (now the tree's `derived_with_job`), renamed
`a_cancel_parked_inside_the_tree_mask_blur_…`, and after the re-run asserts
`mask_cache_bytes > 0` (carried nit 2; the cache is `Arc`-shared across
clones); the parked chain test and the fallback bench take a guided fixture
(`guided_over`: a guided Shadows/Highlights radius 8 at depth 0 on top),
keeping `c_row`.

## 13A-5c-3 contract deltas

13A-5c-3 extends the 13A-5c-2b contract; everything above not restated here
holds. No FFI, header, Swift, MCP, plan-JSON field, `.rz`, tool-table or
README change; `contract_version` stays 1 and only `scratch_heap_bytes`
moves; `work_units`, every refusal (order and sentence) and every
`RenderBudget` charge are unchanged (the 599/600 and 899 budget pins hold);
full-frame and regional OUTPUT are unchanged at 8, 16 and float. The typed
path is untouched.

**The description parse under every profile.**
`adjust_calibrated::parse_scratch(layers)` (NEW) is `context_scratch`'s figure without its
profile gate: the largest non-group `meta` length × 256, `None` past
`MAX_RZDC_META_LEN`; `context_scratch` becomes `Some(0)` under a modelled
profile, else `parse_scratch` (its callers — the cutout, `inpaint_source`'s
Tiles/FullFrame arms, the conversion — are unchanged). `regional_admission`
reserves and plans `parse_scratch` where it used `context_scratch`, because
the walk parses `Adjustment::from_meta` per adjustment node under EVERY
profile: under an unmodelled profile the figure is the one 13A-5c-1 planned
(one parse at a time covers both the preflight's and the walk's), under a
modelled one it is new. The 64 KiB memo only makes a hit cheaper; the miss
is the peak.

**The uncharged nest term.** `doc_regional_budget::materialization_charges`
returns `Option<(rect, charged_peak_pixels, uncharged_peak_bytes)>`; the
budget dry run inside it is byte-identical. The third figure mirrors the
reference's allocation statements inside the materialized nest, guarded
exactly as the charge points are (`visible && !env.cut.excludes(idx)`),
allocating nothing. With `A` the accumulator's pixels, `W` its width, `S =
min(A, W·256)` (one package strip), `L(x)` a raster's pixel area or a
group's `|rect|`: a materialized group node's `synth(g) = 4·|rect| +
[masked]·|rect| + [planes]·style_scratch(..).0` (its synthetic, mask crop
and — previously uncharged for a NESTED styled group — its planes, alive
through the unit); `pkg(x) = [planes ∧ opacity < 1 ∧ ¬Dissolve]·16·S·(1 +
kplanes)` (`composite_package_into`'s strip and `style_knockout::windowed`'s
per-plane strip copies; `kplanes` = `[Shallow plane present]` for a unit
base, `2·[members_want_base_snapshot]` for a run member); `lone(x) = guide(x)
+ pkg(x) + [styled]·([Blend If]·16·min(L, A) + [routed]·(20·A +
[¬planes]·L))`; a unit with an empty positional run is `lone(base)`; with a
run and a base opacity > 0 it is `20·A + guide(base) + pkg(base) +
[a drawn member knocks out]·16·A + [styled base]·([Blend If]·16·min(L, A) +
[¬planes]·L) + max over drawn members of lone(m)` (0 at opacity 0:
`composite_clip_group_into` returns before allocating); a unit's figure is
`Σ synth + max(max nested uncharged, unit buffers)`; a pass-through unit's
is `[masked]·A` (the owned crop `composite_unit_into` takes before the
opacity check, charged at `A` even where it is borrowed) plus, at opacity >
0, its children level's figure; a level is the max over its units.
`regional_admission` adds it: `materialization_scratch += peak·16 +
pixels·4 + mask + planes + uncharged`. It is a CONSERVATIVE upper bound
where stated (the crop at `A`, `pkg` at `S`, `guide(B)` added beside the
later `20·A`) and never under a buffer the reference holds inside the nest.

**Re-stated, not closed.** D5's Background snapshot and an unmaterialized
pass-through level's Shallow snapshot stay a stated difference WITH CAUSE:
the budget bounds memory a nest HOLDS and the regional path holds neither
(`regional.rs`'s STATED DIFFERENCES); a visible zero-opacity isolated group
member of an unmaterialized unit is folded into the "unmaterialized level"
clause.

**Re-based pins.** Every exact `scratch_heap_bytes` pin on a fixture with a
non-group `meta` gains `largest meta × 256` (a per-file
`description_parse(doc)` helper restating the rule: `regional_document`,
`regional_groups`, `regional_masks_clips`, `regional_byte_styles`,
`regional_byte_guide`, `regional_byte_nested_guide`, `regional_byte_clip_guide`,
`regional_byte_pass_through_guide`); `regional_byte_styled_groups`' plan
fixture gains its subgroup A's 6×3 synthetic (`18·4`);
`regional_context_preparation_cancel`'s sRGB twin now carries `META_BYTES·256`
too. Difference-of-two-plans pins were unaffected (both fixtures carry the
same largest meta). `regional_sampling_tree_cancel.rs`' cut-plate
comparisons run under the route-witness limit (carried nit 1) and
`small_group`'s doc comment says it is a covering pin only (carried nit 2).

**The new suite** (`regional_resource_accounting.rs`, 17 default tests):
(i) an sRGB `levels` row charges `len·256`, three rows the LARGEST, a
group's meta nothing, the unmodelled twin the same figure; (ii) materialized
nest fixtures (a colour-overlay nest over a 100×100 frame, so `S = A`)
pinned exactly against hand-restated figures — the plain control (0), a clip
unit with a raster member (`20·A`) and a group member (`+ 4·|rect_m|`), a
knockout member, a Blend If base with a run, a routed lone base, a nested
styled group with and without a mask, a lone package (`16·S` over its
opacity-1 twin) and as a clip base (`20·A + 16·S`), a package member beside
a knockout sibling (`16·S·3` inside the member max), a package under a
Shallow pass-through level (`16·S·2` against the knockout's unit pass), a
guided Shadows/Highlights base (`20·A + guide`), a masked pass-through at
opacity 1 and 0, an unmasked one plain and gated, two siblings (max) and a
nest two deep (sum); (iii) every fixture equals the reference crop and
completes under a scratch limit of exactly its own plan.

## 13A-6 cap survey (recorded in 13A-6a)

Every native-precision coverage cap 13A-6 inherits, by `file:line` (line
numbers as of `39f6a17`, before 13A-6a moved any):

**SELECTION.** `precision_selection_plane.rs:112-116,138` — Feather/Smooth
`n·2·taps` (`taps = nearest_odd(2r + 1).max(3)`), Grow/Shrink/Border `32n`,
Transform `taps²·n`, against `MAX_WORK_UNITS` = 2·10⁹: a radius-250 feather
refuses from 1,996,008 px (`n·1002 > 2·10⁹`), radius 50 from 9.90 MP
(`202n`), radius 20 above 24.4 MP (`82n`); morphology above 62.5 MP.
`precision_selection_refine.rs:34-54` — Refine is the stages' SUM (a
per-stage-under / sum-over refinement refuses). `precision_selection.rs:78`
— source selections `40n` (above 50 MP). `ffi_selection_tools.rs:182` — the
byte selection transform's `MAX_PIXELS`. The BYTE feather
(`doc_select.rs:312`) has NO cap: a depth-parity gap 13A-6a closes for small
supports.

**ARTWORK.** `artwork_coverage.rs:9-11,163,173` (262,144 points / edges,
`MAX_SCAN_WORK` 256M); `:206-212` charges `edges·height + n` because `:222`
scans EVERY edge per row (O(E·H): 58,000 edges refuse at H = 4000, 29,000 at
H = 8000); `ffi_artwork.rs:51-52,57`; `artwork_render.rs:72`
(`samples + intrinsic > 64,000,000`: one canvas plane per component); text
4,096 units `ArtworkTextCapture.swift:9-10,147,367` = P7-34 / 13D.

**LONG STROKES.** `brush_coverage.rs:8-9,44-45,50-72` (50,000 dabs; 256M =
`2n + Σ footprint·(64 hard | 1 soft)`: at 24 MP a hard 512-px brush gets 12
dabs, a 200-px one 81, a soft 512-px one ≈790), `:74` scratch `12n` (a
canvas-sized f64 field, 288 MB at 24 MP), `ffi_brush.rs:21-22`;
`NativeBrushStroke.swift:7,24-28,141,147-149,157-158` (the UI refuses at
50,000, nothing partial); `SoftBrush.swift:153,167` (the BYTE walker
TRUNCATES silently); Quick Mask strokes share it
(`ImageCanvasView+NativeBrush.swift:20`,
`AgentServer+SelectionHistory.swift:40`). RESOLUTION REDUCTION:
`PrecisionStrokeCoverage.swift:28-58` — typed MCP strokes
(`AgentServer+PrecisionPaint.swift:78`,
`AgentServer+PrecisionScalarEditing.swift:55`, `PrecisionColorTools.swift:157`)
rasterize byte CoreGraphics coverage promoted ×257 (`.promotedU8`) and refuse
past 50,000 stamps (`:22-25`); `raw_local_coverage.rs:10-11` (4,096
RAW-local dabs) is 13D's.

## 13A-6a contract deltas

13A-6a touches no regional code: the 13A-5c-3 contract holds unchanged. No
FFI signature, plan-JSON field, `.rz`, tool-table, MCP argument or cap
constant changes; `MAX_WORK_UNITS` is 2·10⁹ as before and no Swift file
moves.

**The work check moves to `apply`, same sentence; `plan` reports scratch
only.** `PlaneOperation::plan` keeps every parameter check (radius finite in
[0, 250]; Refine's `validate`) and the scratch formula (`n·24 + 24·max(w, h)
+ 4096 + staged`, still ≥ the crop path's peak) but no longer charges work
for Feather / Smooth / Grow / Shrink / Border or Refine; Blend, Luminosity,
Transform and the pointwise kinds keep their plan-time checks. `apply` raises
"native selection exceeds the work limit" for the five kinds when
`support::work(op, window) > MAX_WORK_UNITS` — `|W|·2·taps` or `|W|·32`,
today's expression with `n` replaced by the window's pixels — before any
allocation. So `rz_selection_u16_plan` no longer refuses a beyond-cap spatial
request: the refusal surfaces from `rz_selection_u16` (the Swift
`NativeSelectionWork` plans then computes and both calls throw
`RasterCoreError`, so the host sees it the same way).
`ScalarOperation::Morphology` (Channels › Modify Plane) plans scratch through
`plan` and runs `apply`, so it inherits the bounded path and its apply-time
check.

**Support-bounded evaluation** (`precision_selection_support.rs`, index
arithmetic only). `support` is the bounding rect of the NONZERO samples
(`None` → a zeroed plane); `reach` is the Gaussian's half width
`nearest_odd(2r + 1).max(3) / 2` for Feather/Smooth and `ceil(r) + 1` for
Grow/Shrink/Border; the window is the support grown by the reach and clamped
to the canvas; `crop` → `PlaneOperation::spatial_kernel` (today's two
spatial arms, extracted statement for statement — the ONE kernel body) over
the crop's dimensions → `embed` into a zeroed canvas plane. A full-support
input takes the window = the canvas: today's arithmetic on the same plane
behind today's work figure, so everything refused before is refused now.

**Exactness** (the new module's `//!` doc): Feather/Smooth accumulate
`k·sample` in a fixed tap order with edge clamps and no absolute-coordinate
arithmetic; with the window grown by `taps / 2` every clamped tap of the crop
reads a sample that is 0 in both paths (or the same canvas-edge sample), the
horizontal intermediate is exactly 0.0 outside the support's rows, so the
crop is f32-bit-identical inside the window and the full plane is exactly 0
outside it. Morphology: every inside pixel (`≥ 32768`) lies in the support;
a window pixel's nearest outside pixel is strictly nearer than anything
beyond the window (the ring at Chebyshev 1 is inside it, reach ≥ 2); with
exact envelopes each path computes the exact 2-D minimum over its own site
set, the same integer squared distance, exact in f32 below 2^24, and every
code depends only on distances ≤ r + 1 ≤ 251; pixels outside the window are
≥ r + 2 from the support ⇒ 0.

**Refine keeps its summed check, now support-bounded.**
`precision_selection_refine::work_units(support, ..)` charges stage i
(smooth, shift, feather in that order) at `support::work(stage_i, window_i)`,
`window_i` the INPUT support grown by the CUMULATIVE reach of stages 1..=i
and clamped (an upper bound on the window each stage's bounded `apply` runs
over), plus `n` for contrast; `None` support charges only the contrast term;
`Some(canvas)` is the pre-13A-6a canvas sum exactly. `refine` computes the
support once (one extra polled n-scan; the stages recompute their own),
refuses on the sum BEFORE stage 1, then composes through the bounded `apply`
(`compose`, the ONE stage loop; `contrast_curve`, the ONE contrast loop,
moved verbatim). A per-stage-under / sum-over refinement over a FULL support
refuses exactly as before (pinned: a 10 MP select-all `refine smooth 40
feather 40 contrast 0.5` = 2·1.62·10⁹ + 10⁷).

**The EDT envelope is f64** (`doc_select::edt_1d`, the ONE Felzenszwalb–
Huttenlocher body byte Grow/Shrink/Border, byte Refine's shift, `inpaint_em`,
`bayesian_matting` and the typed morphology share). Only the intersection `s`
and the boundaries `z` move to f64 (`edt_squared` allocates `z` as
`vec![0f64; n + 1]`); `d`, `f`, `v`, the `sqrt` and the byte coverage ramp
stay f32 / usize. Numerators are exact in f64 (f32 values plus squares
< 2^33), one rounding per `s`; intersections are rationals with denominator
≤ 2·65535, so two distinct ones differ by ≥ 2^-34 (> 4 ulp below 2^17) and
equal ones round equal: every `s > z[k]` / `z[k+1] < q` decision is the exact
rational one, offset-independent, and each 1-D pass is the exact lower
envelope. The f32 envelope mis-selected by ±1–2 squared px once `f[q] + q²`
reached 2^24 (coordinate ≳ 4096) — which is why a crop and the whole plane
disagreed past 4096 — and is unchanged below that regime; no existing fixture
reaches it (`assisted_replay.rs:471`'s 4096² is a rasterizer-cap refusal),
and `make test` is green unchanged. Pinned: the 9000×8 translation pin fails
on the f32 envelope and passes on f64.

**Cancellation** (13A-5a's `T_f + W_s·c_s`, `T_f` = 1 ms): the support scan,
crop and embed poll every 4096 samples; the kernel keeps its own polls (the
Gaussian per row and per 4096 cells, its stage-6 reports now over the CROP's
rows; the EDT per column / row). Declared bound: a cancel issued inside the
crop's horizontal pass returns within `T_f + 4096·c_cell`, `c_cell` one
output cell of that pass from the worker's own checkpoint timeline.

**The whole-plane reference is retained.**
`precision_selection_plane::reference_apply` (`#[doc(hidden)]`, the
`RzJobControl::park_at` pattern: a black-box seam for tests only) parses the
request, validates as `plan` does, and runs the five kinds through
`spatial_kernel` over the canvas behind the pre-13A-6a canvas work check and
Refine through `reference_refine` (`compose` / `contrast_curve` with
`spatial_kernel` over the canvas behind `work_units(Some(canvas), ..)`); any
other kind refuses "reference covers spatial kinds and refine only".

**The suite** (`core/tests/bounded_selection_support.rs`, 12 default tests
and one `#[ignore]`d bench probe in `make cancel-bench`): every kind at radii
0.5 / 1 / 7.3 / 64 / 250 over random u16 selections (odd codes included)
with supports interior, touching each canvas edge, 1×1, empty and the full
canvas, and seven Refine compositions (shift < 0 and contrast 1 included),
equal `reference_apply` sample for sample; a 4400×4400 canvas with the
support inside [4200, 4300)² for all five kinds at radii 1 / 7.3 / 64 and one
Refine — Feather/Smooth 64 are beyond the cap there, so they take the window
rule below; the envelope pin; beyond the cap — feather and smooth 250 on
B1's 2048×1024 fixture, grow / shrink / border 10 on 8000×8000 (64 MP), and
a 4096×2048 `refine smooth 40 shift -3 feather 40 contrast 0.5` — each
finishes at the canvas dimensions and equals `reference_apply` on a canvas
holding only the window (the cumulative window, reach 40 + 4 + 40, for
Refine), embedded, zeros outside; the full-support twins refuse with the same
sentence (the single-kind plan succeeds); a pre-cancelled token, a cancel
PARKED inside the crop's horizontal pass (the blur's stage-6 reports count the
228-row window's rows, never the canvas's), then a complete run equal to
`reference_apply`.

**Carried 13A-5c-3 items.** (1) `member_env` (`style_knockout.rs:458-471`)
sets BOTH of a clipped member's knockout planes to the unit's base-only
snapshot and never passes the enclosing plane through: without a knockout
member both are `None`, so `windowed` (`style_composite.rs:558`) copies 0
windows; with one, 2. `2·[members_want_base_snapshot]`
(`doc_regional_budget.rs:320`) is therefore EXACT — no code change; pinned by
`a_package_member_under_a_shallow_level_copies_no_knockout_window` (a
drop-shadowed member at opacity 0.5 under an unstyled base inside a plain
Shallow-holding pass-through level, no knockout sibling in the run:
`scratch(package) − scratch(opacity-1 twin) == 16·S`; the Shallow sibling
unit renders planes, so its routed pass is exactly `20·A` and never masks the
difference). (2) Each twin now also carries its ABSOLUTE figure through
`assert_nest`, restated by hand with `drop_shadow_planes(w, h)` (`DROP_SHADOW`'s
`style_scratch`: pad 5, `P·4 + P·32 + 5·4 + 4096 + 16·min(h + 10, 256)·(w + 10)`
= 50,916 over 20×20): the lone package's direct twin `(A, planes)`, the
knockout-free member twin `(A, planes + 20·A + 16·S)`, the Shallow level's
direct twin `(2·A, planes + 20·A + 100)` (the level's snapshot is charged, a
second `A` in the peak) — all three were cheap to restate. (3) The 13A-5c-3
conservative-terms list above drops "`20·A` for a styled base's 16·A +
shape": the code charges `20·A` for every run at opacity > 0, styled or not,
which is the reference's own unit buffer, not a conservative stand-in.

## 13A-6b contract deltas

13A-6b touches no regional code: the 13A-5c-3 contract holds unchanged. No
FFI signature, plan-JSON field, `.rz`, tool-table, MCP argument or cap
constant changes (`MAX_PATH_POINTS` / `MAX_PATH_EDGES` 262,144,
`MAX_SCAN_WORK` 256,000,000); the refusal sentence is unchanged ("artwork
outline exceeds the bounded area-integration work limit"); `ffi_artwork.rs`,
`artwork_render.rs`, `lib.rs` and every Swift file are untouched (the scratch
model is unchanged: the sort is in place and `active ≤ E`). The header gains
a three-line comment under `rz_artwork_path_coverage` (13A-6a's comment-only
precedent).

**The sweep** (`artwork_coverage::rasterize`, signature unchanged; the ONLY
caller is `ffi_artwork.rs:72`, which Swift reaches per `.path` part over the
artwork EXTENT through `RasterCore.swift`'s `RasterArtworkCore.coverage` from
`NativeArtworkEvaluation.swift:94`). `outline`, then `charge(n)` — ONLY `n`
up front — then the edges are sorted IN PLACE by `min y` (`total_cmp`),
UNCHARGED: at most E·log₂E ≈ 18·E comparisons, bounded by `MAX_PATH_EDGES`
like the outline itself. No edges → zeros, one poll, `report(1.0, 5)`.
Otherwise `first = clamp(floor(min top), 0, H)`, `last = clamp(ceil(max
bottom), 0, H)`; rows `[0, first)` (only if `first > 0`) are zeroed with one
poll and `report(first/H, 5)`; each row of `[first, last)` polls, advances a
cursor over the sorted edges pushing those with `top < row + 1`, `retain`s
those with `max y > row`, charges the active count, runs the row body and
reports `(row + 1)/H` at stage 5; rows `[last, H)` (only if `last < H`) are
zeroed with one poll and `report(1.0, 5)`. The active SET per row equals the
full scan's (`min(max y, row + 1) > max(min y, row)` ⟺ `max y > row` and
`min y < row + 1`; an edge ending exactly on the row's top or starting
exactly on its bottom is excluded by both). Work: `n + Σ_rows active_row +
body`, rows outside the outline skipped.

**The row body is shared.** `row_coverage` + `RowScratch` are the old loop
body (the `high <= low` guard, `add_break`, the column-crossing charge, the
`active²` charge, sort/dedup, the `breaks·(active + 1)` charge, the
intervals, `span`, the output push) extracted verbatim minus the active-set
selection; `job.every(i)` runs per ACTIVE edge and per pair-outer. Only the
retained ORDER of the active edges differs (sorted by top, not outline
order), and it reaches no sample: breakpoints are sorted and deduplicated; a
pair's crossing `low − d0·(high − low)/(d1 − d0)` is exact under operand swap
(IEEE negation is exact, `(−a)/(−b) = a/b`, `x − y = −(y − x)`);
intersections are sorted by x and equal-x intersections bound empty spans.
The module doc carries this argument.

**The full-scan reference over a row band.**
`artwork_coverage::reference_rasterize(commands, points, width, height,
even_odd, rows)` (`#[doc(hidden)]`, the `reference_apply` pattern: a
black-box seam for tests only) is the pre-13A-6b scan over rows `[rows.0,
rows.1) ⊆ [0, height)` in ABSOLUTE coordinates: it charges `E·(rows.1 −
rows.0) + width·(rows.1 − rows.0)` up front, selects every overlapping edge
per row in OUTLINE order and runs the same `row_coverage`, under
`JobContext::NONE`, returning `(rows.1 − rows.0)·width` samples; it refuses
`rows.0 > rows.1` or `rows.1 > height`. Over `(0, H)` it is today's full scan
exactly (`E·H + n` then the body).

**Monotonicity: never charged above the reference.** The sweep's total is `n
+ Σ_rows active_row + body` and the reference's `n + E·H + body` (plus 2 per
skipped row), with the same per-row SET of body charges (within a row the
column-crossing charges come in sorted-by-top order, not outline order, and
each row's body total is the same); `Σ_rows active_row ≤ E·H`, so at every `charge` checkpoint the sweep's running total
is at most the reference's and nothing the reference accepts is refused. The
up-front charge is `n` ONLY: an earlier draft charged `n + E`, which breaks
this — an outline whose every edge spans every row has `Σ active = E·H`, and
the reference's total can sit exactly at `MAX_SCAN_WORK`, which `charge`
accepts (`> MAX_SCAN_WORK` refuses). Pinned by the boundary fixture below.

**Charges and reports.** The pair term `active²` and the unpolled inner pair
loop (for `active < 4096` the pair-outer's `every(i)` polls once) are
pre-existing and unchanged — the 18,000-edge 1×1 refusal still fires on both
paths — owner 13D (targets). Reports: one stage-5 report per visited row plus
one per NON-EMPTY skip block (a 64×8000 canvas with ink in rows 3000–3099
reports exactly 102 times, where the full scan reported 8,000).

**`artwork_render`'s 64M aggregate: KEPT.** `samples + intrinsic ≤
64,000,000` (`artwork_render.rs:72`, `ffi_artwork.rs:119`, the Swift twin
`NativeArtworkEvaluation.swift:69`) is a pointwise pass over host-owned
per-component planes of the artwork's OWN extent
(`NativeArtworkEvaluation.swift:20-31`: `drawing.width/height` is the
shape/text raster rect, not the canvas). Regional work cannot shrink input
the host must hand over; bounding a component to its support needs a
WINDOWED component contract (a rect per component in the JSON, the header,
`RasterArtworkCore.render` and the evaluation) — a new open item owned by
13D (targets). Survey correction: the 13A-6 cap survey's "one canvas plane
per component" is one EXTENT plane per component.

**Cancellation** (13A-5a's `T_f + W_s·c_s`, `T_f` = 1 ms): the sweep polls
at every row start, per 4096 active edges, per pair-outer 4096, per interval
and at every report. Declared bound: a cancel issued inside the row loop
returns within `T_f + c_row`, `c_row` one row from the uncancelled progress
log (`logged`), `(t₂ − t₁)/((p₂ − p₁)·H)` between the first logged progress
≥ 0.05 and the last ≤ 0.5, on `rectangle(0.13, 0.27, 4095.71, 7999.83)` over
4096×8000 (2 active edges per row, one interval).

**The suite** (`core/tests/bounded_artwork_coverage.rs`, 10 default tests and
one `#[ignore]`d bench probe in `make cancel-bench`): the product
(`rz_artwork_path_coverage` into a `0x4a4a`-filled output, asserted untouched
on refusal) equals `reference_rasterize(.., (0, h))` sample for sample under
both fill rules over fractional rectangles, a slanted polygon partly
off-canvas, same-direction and reversed holes, a self-intersecting star and
a bowtie, 4-cubic rings; outlines entirely above / below / left / right of
the canvas (and touching row 0 / `y == H`), straddling the top and bottom
edges, endpoints on integer rows and at `y == H`, a 1×1 canvas (covered,
partly covered, missed), an empty path and a horizontal-only path; and an
in-cap "glyph field" of 200 radius-3 256-gon rings (20 × 10 over 1200×900,
E = 51,200, reference total ≈ 110M). Beyond the cap: 1000 rings (4 × 250 on
400×4000, E = 256,000, 257,000 commands, `E·H` = 1.024·10⁹) — the reference
refuses with the sentence, the sweep finishes at 400×4000 and equals the
reference concatenated over 200-row bands (each band `Ok`). The boundary:
7,999 closed vertical subpaths on 16,000×1 (E = 15,998) whose reference total
is exactly 256,000,000 — both paths accept and agree. The pair term:
`phase8e_coverage.rs`'s 18,000-edge fixture refuses on both paths with the
sentence. The reports pin (102). A pre-cancelled token; a cancel PARKED
inside the row loop (`entry_count(stage 5, progress > 0)`,
`cancelled_parked_at(n0 + 1 + 40, ..)` on 64×2000), then a complete run equal
to the reference.

**The drive's design numbers** (`scripts/verify_phase13a6b_bounded_artwork.py`,
the app-path gate; the beyond-cap PROOF is the suite's — the pre-13A-6b app
is not rebuilt): B1 a 16-bit 240×16400 text layer of 40 lines of "S" at size
40, leading 400 — ≈ 40 × 2,500 flattened edges ≈ 10⁵ ≤ 262,144 in ONE part
(disjoint same-colour glyph bounds merge, `ArtworkTextCapture.swift:118`)
over an ≈ 16,000-row extent, `E·H` ≈ 1.6·10⁹ > 256M under the old charge;
B2 its 8-bit twin (the byte fallback); B3 a within-cap 240×2600, 6-line
control.

## 13A-6c-1 contract deltas

13A-6c-1 touches no regional code: the 13A-5c-3 contract holds unchanged. No
FFI signature, plan-JSON key, `.rz`, tool-table, MCP argument or cap constant
changes (`MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 256,000,000); both refusal
sentences are unchanged ("native brush exceeds 50000 dabs or has incomplete
tuples", "native brush exceeds 256 million footprint work units");
`ffi_brush.rs` (its `request()` reserves `plan.scratch_bytes`, the only caller
of `plan` / `rasterize`), `RasterCore.swift` and every Swift file are
untouched. `lib.rs` makes `brush_coverage` a `pub mod` (the suite imports the
seams; `artwork_coverage`'s precedent), so `rasterize` — whose `JobContext`
is crate-private — is `pub(crate)` like `artwork_coverage::rasterize`.
`artwork_coverage::distance` is `pub(crate)` (the ONE point-to-segment
distance, reused by the exterior test). The header gains a comment under
`rz_brush_u16_plan` (comment only).

**The footprint field.** `footprint(dimensions, dabs)` (`#[doc(hidden)]`) is
the union `[x0, y0, x1, y1)` of `bounds` over the dabs with `d[2] > 0 &&
d[3] > 0` whose clipped box is non-empty, `None` when that union is empty.
`rasterize` runs `plan` (the cap), allocates a `vec![0f64; |F|]` field (polled
`every`), and per dab: skips zero dabs; `bounds`; per row `check()`; per
pixel `every(x)`; accumulates `old += c·(1 − old)` in dab order at `(y −
y0)·Fw + (x − x0)`; reports `(di + 1)/count·0.9` at stage 3 as before. The
output is canvas-sized (`reserved_vec(n)`, polled `every`): the field's
quantization inside F, 0 outside — exactly the reference's quantization of
its untouched zeros.

**One per-pixel expression.** `evaluate` is the pixel-mode overlap / soft
centre sample / hard 8×8 sum of the pre-13A-6c-1 body, arithmetic and order
unchanged, before the dab's opacity; `raw_local_coverage::radial` stays the
ONE coverage evaluator. `reference_rasterize(dimensions, dabs)`
(`#[doc(hidden)]`) is the pre-13A-6c-1 body — canvas field, every dab's box,
`evaluate` on every pixel, `JobContext::NONE`, geometry validated through the
shared `validate_dabs`, NO work cap — the suite's oracle.

**Exact classification** (hard dabs — hardness 1, not pixel mode — with
`d[2] ≥ 1`; sub-1-px hard dabs run `evaluate` on their ≤ 2×2 box). The four
pixel corners are mapped with `sample`'s own expressions into the normalized
frame. INTERIOR iff every corner has `hypot ≤ 1 − HARD_EDGE_MARGIN` (convexity:
all 64 samples return 1.0, coverage `1.0·d[3]` bit-identical to
`64.0/64.0·d[3]`). EXTERIOR iff (a) the dab centre is NOT in the closed pixel
square (comparisons only; the affine bijection makes it the
point-in-parallelogram test) AND (b) the min over the parallelogram's four
edges of `artwork_coverage::distance` from the origin is `≥ 1 +
HARD_EDGE_MARGIN` (every sample then returns 0.0 and the field write is an
identity, so it is skipped). Otherwise BAND: `evaluate`. `HARD_EDGE_MARGIN` =
2^-16 covers twice the normalized-distance error bound over the validated
domain (coordinates ≤ 1e8, minor radius ≥ 0.005: `|Δq| < 2.2e-6`, `|Δd| <
3.1e-6`); the module doc states the bound, its domain and the exactness
argument.

**Charges.** `work_units = n + |F| + Σ_dabs charge`: zero dabs 0, pixel/soft
`box`, hard `min(64·box, box + 64·band_bound(d[2]))`, with `band_bound(d) =
ceil(4πaρ + πρ²)`, `a = d/2`, `ρ = √2 + HARD_EDGE_MARGIN·a` (the module doc's
Steiner bound on the band; `band_count` is the `#[doc(hidden)]` classifier
count the suite pins against it). `scratch_bytes = 4·BOOKKEEPING_BYTES +
8·|F| + 2·n + 8·dabs.len()`. `n + |F|` is charged up front and the per-dab
overflow check is kept: `|F| ≤ n` and every dab's charge is at most its old
`box·(64 | 1)`, so at every dab prefix the running total is at most the old
one (the old plan charged `2n` up front) — nothing the old plan accepts is
refused. B1's 40-dab 1600×1200 stroke: old ≥ 675M, new ≤ 26M; a hard 512-px
dab costs ≥ 554,560 (`512² + 64·4569`), so the new cap admits ≈ 454 of them
fully on canvas where the old admitted 15 at 1600×1200.

**Cancellation** (13A-5a's `T_f + W_s·c_s`, `T_f` = 1 ms): the dab loop polls
at every dab start, every dab row, `every(x)` at `x & 4095 == 0` and at every
report. Declared bound: a cancel issued inside the dab loop returns within
`T_f + 64·(x1 − x0)·c_eval` (one dab row entirely band, the worst polling
gap), `c_eval` from the uncancelled `logged` run: `(t₂ − t₁)/(dabs between ·
E_dab)`, `E_dab = box + 64·band_count`, between the first logged progress ≥
0.05 and the last ≤ 0.5, on a hard 512-px round tip, 120 dabs 8 px apart
along a diagonal, fully on 1600×1600, `bare_edit` into a caller-owned
sentinel output (13A-6b's harness rule).

**The suite** (`core/tests/bounded_brush_footprint.rs`, 17 default tests and
one `#[ignore]`d bench probe in `make cancel-bench`; through
`rz_brush_u16_plan` / `rz_brush_u16` into a `0x4a4a` output asserted untouched
on refusal; oracle `reference_rasterize`; every exactness case also asserts
the plan's `work_units` / `scratch_bytes` ≤ the test's own old formulas `2n +
Σ box·(64 | 1)` / `4 MiB + 12n + 8·len`): hard dabs at diameters {0.6, 1,
1.2, 2, 5, 15.8, 66.6, 256, 512} × roundness {0.01, 0.37, 1} × angles {0, 30,
45, −77.5, 180} × centres (generic sub-pixel, an exact pixel centre, an exact
pixel corner) × `d[3]` {1, 0.4}; the two NAMED exterior-rule regression cases
(`hard_sub_pixel_dab_inside_one_pixel_is_band`,
`hard_needle_inside_one_pixel_is_band`); pixel mode, soft (hardness 0, 0.5),
request opacity 0.6; 40-dab overlapping strokes forward and reversed; 1×1,
3×2, 64×64 and 257×129 canvases with dabs straddling every edge and corner,
entirely off canvas, zero-diameter and zero-opacity; nothing to draw
(`footprint` `None`, zeros, work `n`, scratch `4 MiB + 2n`); the footprint
union; beyond the cap — 18 hard 512-px dabs on 1100×1100, old formula ≈ 305M
> 256M, the product == reference at 1100×1100; a soft 1000-px 260-dab stroke
and 50,000 hard 512-px dabs still refuse with the 256M sentence (plan and
product); `band_count ≤ band_bound` over 15 diameters (0.3 … 1200) × roundness
× angles × offsets; `scratch_bytes` pinned on three fixtures; a pre-cancelled
token; a cancel PARKED inside the dab loop (`entry_count(stage 3, progress >
0)`, `cancelled_parked_at(n0 + 1 + 40, ..)`), then a complete run equal to the
reference.

**The drive's design numbers** (`scripts/verify_phase13a6c_bounded_strokes.py`,
the app-path gate; the route `nativeStrokeArguments` → `precisionPaint` →
`capturedNativeStroke` → `NativeBrushStroke.coverage` → `rz_brush_u16_plan` /
`rz_brush_u16` is cited in its docstring): B1 a 16-bit 1600×1200 hard 512-px
`stroke_version: 1` stroke, spacing 5, (300,600)→(1300,600) — 40 dabs fully on
canvas, old charge ≥ 675M, new ≤ 26M — black at (800,600)/(800,350), white at
(800,200)/(43,600); B2 the within-cap 2-dab control (spacing 50, old ≈ 37M);
B3 the 8-bit twin (byte RZBR replay; visible parity only); B4 the beyond-NEW-
cap refusal — 14 points cycling the rectangle (300,300)–(1300,900), ≥ 531
fully-on-canvas dabs at ≥ 554,560 each, Σ ≥ 294M — must refuse with the
core's own "256 million footprint work units" and leave the document white,
which proves from one build that the tool route reaches `rz_brush_u16_plan`.

## 13A-6c-2 contract deltas

No FFI signature, header, `RasterCore.swift` wrapper, plan-JSON key, `.rz`,
tool-table, MCP argument or cap NUMBER changes (`NativeBrushStroke.maximumDabs`
and `SoftBrush.maxStampsPerStroke` 50,000 — the latter now defined AS the
former — `MAX_BRUSH_DABS` 50,000, `MAX_BRUSH_WORK` 256,000,000); the core's
two sentences are unchanged and reach the user verbatim.

**One Swift sentence.** `NativeBrushStroke.dabLimitRefusal` = "This stroke
exceeds the native brush limit of 50,000 dabs; no partial stroke was applied.
Use a shorter stroke or wider spacing." It is thrown by `init(tip:pixel:dabs:)`
for more than 50,000 dabs (the tip/empty/tuple guard now says "Invalid native
stroke tip or dab tuples; no partial stroke was applied."), by
`init(canonical:)` for a `dabs` text above `maximumDabs·44` bytes or a decoded
payload above 50,000 dabs, set by the session's `stamp` / `pixelSegment`, and
thrown by `SoftBrush.stampCenters` at the point it used to return a truncated
prefix (walk arithmetic unchanged). Every byte MCP caller (`paintStroke`,
`healStroke`, `spotHealStroke`, `cloneStamp`, `dodgeBurn`) computes the walk
before its non-throwing draw closure, so an over-cap stroke refuses before any
overlay is drawn; the two local-mask gesture views catch it into their own
`refuse`.

**THE CONTRACT DELTA: unversioned 16-bit typed point strokes.**
`brush_stroke` / `eraser_stroke` / `clone_stamp` / `heal_stroke` /
`spot_heal_stroke` (layer RGB), the mask/plane/channel scalar targets and
`dodge_burn` at 16 bits WITHOUT `stroke_version` / `native_stroke` no longer
rasterize the ×257-promoted byte footprint (`PrecisionStrokeCoverage.stroke`,
deleted, with `EditorViewController.precisionHealBounds`). They generate
native dabs through `NativeBrushStrokeSession.generated` (unit pressure, no
timing) and rasterize through `rz_brush_u16_plan` / `rz_brush_u16`, the
`stroke_version: 1` commands (`paint_blend`, `erase_native`, `.spotHeal` /
`.retouch` with `native.healBounds`, `dodge_burn`) with the selection joined as
an independent native gate. A default (hard) tip that used to stroke an
uncapped byte path, and a stamped tip that used the dab-capped adapter, can
now refuse with the core's "native brush exceeds 256 million footprint work
units" as well as the dab sentence. T4's numbers: an 81-point size-512 hard
stroke alternating (100,600)/(1500,600) stamps 801 dabs at 128-px spacing
(≪ 50,000); every dab box is ≥ 356×512 px and a classified dab is charged
`pixels + 64·band_bound(512)` = `pixels + 64·4,569` ≥ 474,688, so Σ ≥
380,225,088 > 256M and the stroke refuses by name with the document
unchanged. The reply's `coverage` for such a call reads "Native version 1 dab
coverage generated from points; source RGB stays u16." 8-bit unversioned
strokes are untouched.

**Core nits.** `BrushRequest::plan` runs the tuple-count check (the private
`check_count`, shared with `validate_dabs`) BEFORE `footprint`, so a 50,001-dab
or 7-value list refuses with "native brush exceeds 50000 dabs or has
incomplete tuples" without first walking the list; `band_count` returns 0 for a
slice shorter than one dab. Pinned by
`plan_checks_the_dab_count_before_the_footprint`.

## 13A-7 contract deltas

No FFI signature, header, `RasterCore.swift` wrapper, plan-JSON key, `.rz`,
tool-table, MCP argument, catalog description or cap changes; no core
behavior change.

**The single-instance drive.** `scripts/verify_phase12e2b_placement.py` runs
P1–P10 in ONE owned `HostApp` (the per-gate relaunch is removed, as the
12E-2b "Open, with owners" acceptance asks) and adds **P11**: the count of
threads PARKED in `-[NSAnimation _runBlocking]` (in that frame for the whole
one-second `sample`; raw frame lines recorded, not asserted) in a `sample` of
that instance is read once the instance holds its first document (the
starting value — where 12E-2b's measurement read it), after P10, and after every 20 of 200 more
consecutive worker-backed edits (`place_image` + `undo` on a fresh 8-bit
fixture stack, each presenting the `BackgroundWork` panel). Fixed before
measuring: every reading EQUAL to the starting value and no sample naming the
Dispatch Thread Soft Limit. The samples are kept under the receipt's
`work/panel-threads/`; `receipt.json` is now written even when a gate fails.

**Carried 13A-6c-2 nits.** (1) The UNVERSIONED scalar (mask / plane /
channel) stroke reply's `coverage` reads "Native version 1 dab coverage
generated from points; the scalar target stays u16." (it said "source RGB
stays u16", which does not apply to a scalar target; the "generated from
points" substring the 13A-6c-2 drive matches is unchanged; the layer-RGB
reply is unchanged). (2) `retouchUsesDabs` runs on a heal / clone call only
when it carries no `native_stroke` — its result is discarded and it only
validates `footprint`, which a captured native stroke does not read; that is
the pre-13A-6c-2 behavior (the call sat in the `native == nil` arm). (3)
`BrushRequest::plan`'s early `check_count` carries a comment naming it the
ordering guard (`validate_dabs` repeats it); comment only.

## Decisions

- **One implementation per algorithm.** The per-pixel bodies were EXTRACTED,
  statement for statement, into `doc::composite_layer_sample` (from
  `composite_layer_into`), `doc::adjust_backdrop_sample` (from
  `composite_adjustment_into`), `precision_render::composite_raster_sample16`
  and `adjust_backdrop_sample16` (from `render_layers`' Raster and Adjustment
  arms). The full-frame loops call them; the regional loops call them with
  coverage 1.0 (unmasked), `k = opacity` and guide 0.0 (pointwise). The new
  modules hold index arithmetic only. The typed linear-light re-encode loop and
  the stores (`from_float_samples` / `from_rendered_float`) are the full
  render's own. `make test` passed in full after the extraction and before any
  regional code existed (2222 tests), which is the byte-identity check.
- **`work_units`** is `ow·oh·(contributing + 1)`: the accumulator fill plus one
  pass per contributing layer, at both depths (the typed `Progress` total, so
  `zero_plane`'s advance stays inside it).
- **Typed skip.** The typed walk skips on `visible` alone, the scene builder's
  own contribution rule.
- **11E.** The transfer comes from `RzDocument::transfer` /
  `PrecisionDocument::transfer`; the regional render adds **no new operator,
  so there is no policy-table entry.** At float, `adjust_backdrop_sample16`
  keeps `apply_linear`'s `Err`.
- **Tool tables.** No new tool name: `render` is already in
  `DocumentPrecision.commonTools` and `FloatDocument.tools` — **no table
  change.** `render` is read-only, so no Actions or history change.
- **Format.** No `.rz`, PSD or `rz_image_render_*` change.
- **MCP.** `render` gains optional `region` `{x,y,width,height}` and
  `preview_divisor` (integer ≥ 1). Either one selects the regional path
  (region defaults to the full canvas, divisor to 1); beside `layer` or
  `channel` it is refused ("region and preview_divisor apply to the flattened
  canvas only"). The byte path passes `RasterResources.shared.workingLimit` as
  the output budget; the typed path renders at native depth and then displays
  through `RasterDocument.display(of:context:)`, the full render's display
  step. The reply text appends ", region x,y w×h at divisor d" and " (preview)"
  when d > 1. The canvas display (`refreshProjection`) is untouched.

- **13A-4 split (13A-4a).** 13A-4 as planned (guided operators at every
  placement, live filters, global operators) was too large for one run:
  13A-4a admits the ROOT placement; 13A-4b owns groups, clip members and
  materialized subtrees (per-level stages, the group's own buffer origin as
  the lattice anchor).
- **Live-filter stacks (13A-4a decision, no code).** A binding's result is
  baked into `layer.pixels` / the typed record at EDIT time
  (`doc_retained_source.rs`), so the document compositor never evaluates a
  filter stack at render time and the regional path already reads the same
  pixels exactly. The live-filter PREVIEW surface (`live_filter_preview.rs`)
  is not the document compositor. Re-evaluating a source per tile is 13B's.
- **Global operators (13A-4a decision, no code).** No document-compositor
  operator performs a whole-frame pixel analysis: Auto Tone / Contrast /
  Color are commands (`ops_auto.rs`) and `selected_auto_levels.rs` is a
  destructive edit; the `document_v2` calibrated context is a description /
  ICC preparation already raised identically by plan and render
  (`validate_context`, 13A-1). Charging description / ICC preparation scratch
  is 13A-5's (resource accounting).
- **Guided Shadows/Highlights (13A-4a).** Staged exactly as the 13A-4a
  contract deltas state; the byte guide's `blur_plane` is uncancellable (the
  reference's own preparation), owned by 13A-5 beside the mask blur and
  `rendered_group`. `ResolvedStep` keeps its node inline under a documented
  `#[allow(clippy::large_enum_variant)]` (the new `layer_units` and `guide`
  fields grew `ResolvedNode`).
- **13A-4b split (13A-4b-i).** 13A-4b as planned (groups, clip members,
  materialized subtrees) was too large for one run: 13A-4b-i admits isolated
  levels and materialized subtrees; 13A-4b-ii owns clip members (the
  accumulator-aligned unit buffer, the base-only backdrop held as a plane)
  and pass-through levels (the enclosing accumulator, the gated copy, the
  Shallow snapshot plane).
- **Materialized precedence (13A-4b-i).** `Enclosure::Materialized` is
  STICKY: every level below a materialized group — isolated or pass-through —
  is `Materialized`, never recorded and never planned, because the render
  never enters a materialized node (the typed tree prepares no children for
  it; the byte driver never descends through `Source::Materialized`). A guide
  there is admitted unstaged; a guide directly in a pass-through level there
  is admitted too (the reference renders it).
- **Clip members refused inside materialized subtrees too (13A-4b-i).** The
  clip-member test precedes the enclosure test, so a guided clip member
  refuses everywhere, materialized subtrees included, keeping one rule for
  13A-4b-ii to lift. (Lifted by 13A-4b-ii-b-1: inside a materialized subtree
  a guided member is admitted unstaged; a styled base refuses by its own
  sentence.)
- **The nested last stage at divisor 1 (13A-4b-i).** A nested level's final
  plane is held at divisor 1 over window ∩ buffer even for a preview — the
  enclosing level reads it at arbitrary samples; sampling it at the preview
  divisor is 13B's.
- **13A-4b-ii split (13A-4b-ii-a).** 13A-4b-ii as planned (clip members and
  pass-through levels) was too large for one run: 13A-4b-ii-a admits a guide
  directly inside pass-through levels; 13A-4b-ii-b owns clip members (the
  accumulator-aligned unit buffer, the base-only backdrop plane, the three
  member loops and the per-strip PACKAGE path).
- **Pass-through levels are absorbed, not staged (13A-4b-ii-a).** The
  reference composites a pass-through group's children onto the enclosing
  buffer (or a same-origin copy), so its guide shares the enclosing level's
  lattice and clamp; staging it as its own level would need a second buffer
  the reference never has. The group's own samples are carried by cursors
  and HOLDS instead.
- **Byte holds only where read; typed always (13A-4b-ii-a).** At 8 bits the
  sample as a pass-through group's children began is held exactly when the
  reference reads it (`pass_through_holds`: the gate or a visible Shallow
  child) — a plain group without a Shallow child costs nothing; the typed arm
  always gates a copy back, so every typed pass-through on a path is held.
  Hence "nested pass-through in pass-through" holds ONE plane at 8 bits when
  one of the two is plain (two when both are gated), and TWO at typed depth.
- **Conservative hold figures (13A-4b-ii-a).** Every hold is charged
  `16·|R_0|` for the whole render even when it is entered over a later, smaller
  region or finished early; the byte hold fill polls but reports no progress
  (the byte `plane` never advances `done`), so the byte fraction under-reports
  by the hold work until the final `report(1.0)`.
- **13A-4b-ii-b split (13A-4b-ii-b-1).** 13A-4b-ii-b as planned (every
  member loop and the package path) was too large for one run: 13A-4b-ii-b-1
  admits a guided member of an UNSTYLED base (the plain private-buffer loop at
  both depths); 13A-4b-ii-b-2 owns a STYLED base's direct and routed member
  loops and the typed package.
- **The unit is RE-RUN, not held (13A-4b-ii-b-1).** A later fill cannot
  resume a unit from the stage plane, which holds the unit BUFFER at the
  cursor (the guide's input), not the accumulator; it re-runs the unit from
  the held "began" sample instead (the base and every member, their guides
  set). The cost is charged — a member stage counts its rows from the unit's
  first row (objection 1 of the spec review) — rather than holding a second
  plane per member.
- **Byte PACKAGE path — refuse by name, owner 13B (13A-4b-ii-b-1, carried
  item 3).** `composite_package_into` (`style_composite.rs:479-540`) runs a
  packaged base's members per 256-row STRIP, so a member's guide is built
  from a strip-anchored, strip-clamped buffer no regional level has; staging
  it per strip is tiled-backing work (13B). A packaged base is a styled base,
  so it refuses "is Shadows/Highlights with a guide under a styled base". The
  TYPED package is one canvas-sized copy (`precision_style_render.rs:28`, no
  strips) and is 13A-4b-ii-b-2's.
- **Styled bases re-run through the STYLE's statements (13A-4b-ii-b-2).**
  The stop is threaded through the existing styled bodies rather than
  writing a styled twin of `sample_clip_until`: every clip run goes through
  `sample_clip_until` / `clip_unit_sample`, which dispatch a styled base to
  `styled_sample` / `styled_sample16` with the member cursor, so the styled
  unit is re-run from the held began sample by the very statements the
  unstaged render uses. The routed unit pass keeps its early-return-first
  order for `stop = None` (objection 2 of the spec review: hoisting the unit
  ahead of the rect test would re-run every routed base's member loop at
  every out-of-rect sample of a plain 13A-3c knockout render).
- **The 8-bit PACKAGE keeps a refusal of its own (13A-4b-ii-b-2).** The
  refusal narrows from "any styled base" to exactly `Styled::of`'s `package`
  (planes, opacity below 1, not Dissolve) with a new sentence naming it, so
  a user can see why an effect-bearing base at full opacity renders and the
  same base at 0.6 does not; the typed package is admitted.
- **`is_pointwise` stays the predicate (13A-4b-ii-b-1).** The walks test a
  member's `!is_pointwise()` before reading its `guide_radius()` (the two
  agree), so the admission question the contract names keeps its one home.
- **Typed buffer = canvas (13A-4b-i).** Every typed accumulator is
  canvas-sized, so a typed level is staged over any window (a window that
  misses the group's content still stages it, asserted); only the 8-bit rect
  can be empty.
- **The cancellation model is per-stage (13A-5a).** A bound is `T_f +
  W_s · c_s`, never a sample count divided into a time that is mostly the
  fixed return floor: the hold fills' 19,900–91,000 "samples" misses of
  13A-4b-ii-a … 13A-4b-ii-b-2 were the ≈0.01 ms floor over a 0.3 ns fill
  sample, not fill work. `T_f` = 1 ms is declared, not fitted (the
  hold-fill floors measured since 13A-4b-ii-a are 0.008–0.034 ms). The 13A-4a / 13A-4b-i printed
  sentences and `regional_byte_guide.rs`'s declared bound are left as they
  were — still true, now looser, upper bounds; their records stand.
- **Checkpoints-only ranges, no byte plan change (13A-5a).** The byte plan
  counts no preparation work, so the new byte checkpoints report nothing
  (`JobContext::checkpoints_only`) and every existing byte progress point
  holds; the byte preparation PROGRESS (plan work for the mask blur, planes
  and materialization, the guide's blur inside a range on the typed
  pattern) is 13A-5b's, with the re-basing it brings. The narrowing lives at
  the CALL SITE, not inside `impl GuideSample for u8`, so the two impls stay
  symmetric and 13A-5b changes one line.
- **Cache on success only (13A-5a).** The derived-mask cache is an `Arc`
  shared by every clone of a document, so a half-blurred plane written on
  cancel would poison every later render of the document and its clones;
  `derived_with_job` writes the cache only after the blur returns `Ok`.
- **The factor > 1 mask blur's interval is one Triangle resample
  (13A-5a).** `image::imageops::resize` is not polled inside; replacing it
  with a polled resampler would change the mask's bytes (and add a second
  resize implementation). Measured ≈57–61 ms (the up pass) on the
  3000×2000 fixture at feather 40 — the declared interval, not a miss.
- **No plan field, no drive (13A-5a).** A plan-level cancellation
  declaration is 13B's plan surface; MCP `render` passes no job control
  (`AgentServer.swift`), so the app-visible behaviour is unchanged and no
  new MCP drive is owed — the regression drives were re-run.
- **13A-5b split three ways (13A-5b-i).** The planned 13A-5b (three
  preparation stages, byte progress accounting and the re-basing of every
  byte probe table) is three runs: 13A-5b-i the style plane render and the
  guards, 13A-5b-ii `rendered_group` (a job through the byte reference
  compositor, which carries none), 13A-5b-iii byte preparation PROGRESS and
  the probe re-basing.
- **Checkpoints-only at the call sites (13A-5b-i).** `Styled::of`,
  `prepare_style` and `composite_styled_into` narrow the job, never
  `render_effects_with_job` itself: the byte plan counts no plane work, and
  the typed style's `advance` stays its one report (a range on the
  `precision_regional_guide` pattern moves typed probes — 13A-5b-iii's).
- **Cache on success only (13A-5b-i).** The plane cache is an
  `Arc<LayerStyle>` shared by every clone; `rendered_with_job` returns every
  `Err` before `store`, and `settled` adopts into a local before writing, so
  a cancelled render leaves no entry half-rendered or half-adopted (measured:
  the cache-integrity test).
- **Per-row polling of the bespoke loops (13A-5b-i).** Without it the
  declared bound would be false for four renderers (a bevel's `shade` is a
  whole-plane loop of float work); the rewrites are row loops over the same
  pairs in the same order, bytes unchanged (`make test` 2598 before the new
  file). Satin's loop uses `chunks` (not `chunks_exact`) so a ragged tail
  would still be visited, matching the whole-plane zip.
- **The one-unit list (13A-5b-i).** `Shape::of_layer`, `Shape::inverted`
  (unpolled; its five callers poll before it), the coverage `clone` /
  `to_vec`, the all-zero scans, `shift_plane`, `multiply_planes`,
  `subtract_planes`, `invert_plane` and `push_banded`'s multiply are each one
  memory-bound pass, bounded by `c_map`; polling them per row is not this
  run's.
- **The job-less names stay (13A-5b-i).** `blur_plane`, `dilate_plane`,
  `erode_plane`, `render_effects` and `render_effect` are uncalled once every
  renderer and the cache poll; they are kept (`#[allow(dead_code)]`, a
  comment naming why) as the primitives the module docs name, and the three
  plane primitives are not re-exported by `style_render`.
- **13A-5b-ii split two ways (13A-5b-ii-a).** The chain (a job in the env,
  per-row polls in the chain's own loops, every other primitive ONE unit,
  every child mask through `active_mask_with_job`) and the carried probe
  fix are 13A-5b-ii-a; per-row polling INSIDE the style/knockout primitives
  (which carry no env and have typed callers) and moving 13A-5b-i's time
  probes are 13A-5b-ii-b.
- **The job rides in `CompositeEnv` (13A-5b-ii-a).** Every chain function
  already takes the env, so a field changes no signature it does not have to,
  and `NONE` in the two literals makes every full-frame caller byte-identical
  by construction. The one live entry is `resolve_materialized`, narrowed at
  the CALL SITE (13A-5a's rule).
- **Warm the child mask, do not re-plumb it (13A-5b-ii-a).** Instead of a
  `job` on every `active_mask()` reader (`Shape::of_layer` and the pixel
  pass carry no env and have typed callers), `composite_styled_into` warms
  the aligned cache under the job at its entry; the mask plane is a pure
  function of the layer, so warming earlier changes no byte, and
  `resources.rs` evicts both caches together, so a warm never blurs where the
  un-warmed path would have hit. Full-frame cost: one mutex probe per styled
  layer.
- **The trigger is the checkpoint count (13A-5b-ii-a).** A time probe
  calibrated on separate runs misfires under load (5/12 in 13A-5b-i's
  rounds); the count is advanced by the worker itself, so the cancel always
  lands after the entry and within the poll gap after a checkpoint — the
  model's worst case. The test reads it through `RzJobControl::checkpoints`
  (`#[doc(hidden)]`, no FFI; the probe is a Rust test seam, not a host
  surface).
- **Mask-cache inspection through the resource report (13A-5b-ii-a).**
  `MaskProperties::cached_planes` is `pub(crate)`; the black-box test reads
  the same planes through `rz_resources_report`'s `mask_cache_planes`, which
  is built from it.
- **Primitives take a `job`, not an env (13A-5b-ii-b).** The six primitives
  carry no `CompositeEnv` and have typed callers, so each gains a trailing
  `JobContext` in place rather than a job-less wrapper (`Shape::of_layer`'s
  three callers all carry a job); only `quantize` / `encode_projection` keep
  their job-less names (nine and many callers, 13A-5b-i's decision). The
  typed sites pass `NONE`: the typed materialization is polled per layer and
  per 4096-pixel chunk (13A-5b-i) and its intervals are not re-based here.
- **No hoisted snapshot (13A-5b-ii-b).** A `?` cannot sit inside the
  tuple-returning Blend If closure, and hoisting `BackdropSnapshot::of` above
  the filter would copy the layer rect for an absent or identity Blend If;
  the closure returns `Ok((b, …transpose()?))` and the outer `Option<Result>`
  is `transpose()?`'d.
- **(m2) re-pointed (13A-5b-ii-b).** 13A-5b-ii-a's (m2) froze in
  `encode_projection`, which is now polled per row; the freeze probe moves
  to (g′), whose gated pass copies the accumulator unpolled
  (`acc.to_vec()`), with the bound that copy declares (`T_f + c_copy`) and a
  freeze no longer than half of it.
- **(q) measures `Shape::of_layer` inside the chain (13A-5b-ii-b).** Its rows
  in the STYLE PLANE RENDER precede `at_the_blur`, so no style-file probe
  targets them; (q)'s child B (a fill-only clip base) runs it inside
  `rendered_group`, where the four row probes land.
- **13A-5b-iii split three ways (13A-5b-iii-a).** The staged path's
  progress and the carried probe items are 13A-5b-iii-a; the unstaged path's
  progress is 13A-5b-iii-b; the per-blur and per-effect sub-ranges and the 7
  typed `NONE` sites are 13A-5b-iii-c (the plan table above).
- **The unstaged fill lands with the ranges (13A-5b-iii-a).** A reporting
  unstaged accumulator fill under `fetch_max` would erase the stage-1 → 6
  flip the four cancel suites' entries (`at_the_blur`, `inside_the_chain`)
  read; only a per-stage range restores an observable entry, so the fill's
  reporting and the plan's mask / plane / materialization work land together
  in 13A-5b-iii-b.
- **A ranged job in `render_staged` only (13A-5b-iii-a).** 13A-5a's
  "`checkpoints_only` at the call site" stands at every other call site;
  `job.range(start, end)` around the byte guide build is the only new
  narrowing, on the `precision_regional_guide` pattern (the range is the
  guide's own plan work, so the advance after it is unchanged).
- **Calibration by checkpoint timeline (13A-5b-iii-a).** The model's "`c_s`
  calibrated on its OWN uncancelled run" for the two freeze probes is the
  worker's own checkpoint timeline, recorded before the probe: the unit a
  cancel froze in is identified by the count it was issued at
  (`Timeline::gap_at(p.issued)`), not by a separate timing on the test
  thread over other memory. The (m2) timeline is recorded TWICE and the
  second kept: the first run pays the gated pass's `acc.to_vec()` on cold
  pages (7.3–9.0 ms measured when the materialization test runs alone,
  against ≈2 ms in the probe's run), so its `longest / 2` freeze never fired
  inside the chain (5/5 alone, and the first serial pass: "the poller
  stalled past the stage … progress 1, stage 6"); the second run is under the
  probe's own warm conditions (a deviation from the spec's one timeline,
  recorded in the implementer's notes).
- **The guide's two blurs share one range (13A-5b-iii-a).** `adjust_guide::
  build` runs two grid blurs under the one range, so progress ramps to the
  range's end in the first and holds it through the second and the combine;
  per-blur sub-ranges inside `build` (shared with the typed driver and the
  full-frame `tone_plane_with_job`) and the style planes' per-effect
  sub-ranges are 13A-5b-iii-c's.
- **The load-robustness policy (13A-5b-iii-b-1; the orchestrator's
  decision, delegated, recorded verbatim).** "1. Default `make test` must
  contain only LOAD-INDEPENDENT assertions: the cancel returns "operation
  cancelled"; nothing partial is cached; output after a cancelled-then-complete
  render equals the reference; progress never exceeds its stage's range end
  and ends where the plan says; report-log checks that a stall can only make
  MISS a leak, never fail a correct render. 2. Every assertion whose verdict
  depends on wall-clock timing or on observing a short-lived value
  (latency-vs-bound verdicts, "first value observed", "strictly inside the
  range" seen by a poller, freeze probes, calibrated MISS/PASS lines) moves
  behind `#[ignore]` (or an equivalent opt-in) and runs via a new `make
  cancel-bench` target (release, `--ignored --test-threads=1`, the regional
  cancel suites), whose output is the measurement record in the status doc.
  Bounds stay declared before measuring and are never widened. 3.
  Verification for this and later runs: `make test` twice plus once in 4×
  parallel rounds must be green; `make cancel-bench` once serially, with its
  PASS/MISS table recorded honestly."
- **Printed-only lines outside the four suites stay default
  (13A-5b-iii-b-1).** The seven non-guide byte concurrent-cancel tests, their
  u16 twins and the five byte and five typed guide `measure` tables PRINT
  PASS/MISS and assert nothing timing-dependent, so they stay in `make test`;
  their printed lines remain their own record.
- **One residual in the default cache-integrity and INTERVAL / PIN tests
  (13A-5b-iii-b-1).** A poller stalled past the WHOLE render still fails,
  naming the stall — in `probe`'s entry wait (`probe.rs`, "returned before
  the entry", the same code for every `Trigger`, so `After(0)` cannot remove
  it) or at the verdict ("completed before the cancel"). Such a failure is
  re-run serially by name and recorded, never chased; a blocking checkpoint
  seam on `RzJobControl` (`#[doc(hidden)]`, parking the worker at count N)
  would remove it — decided in 13A-5b-iii-b-2.
- **The pre-cancelled `took` bound is printed, not asserted
  (13A-5b-iii-b-1),** at all four sites: the return floor is the bench's
  measurement, not a default-test verdict.
- **`assert_ramp`'s whole-range clause precedes its bounds clause
  (13A-5b-iii-b-1),** so a whole-range miss reports before any scan; it names
  a stall unambiguously only when the first value above the start is past the
  advance (`> end + 1e-6`); a first value AT the advance names both the stall
  and the `checkpoints_only` regression and demands a serial re-run. The
  bounds clause can still fail on a missed range END — bench-only, recorded.
- **The bench is `--no-fail-fast` (13A-5b-iii-b-1),** so one binary's panic
  never hides the others' record; its pass criterion is "every binary ran,
  every line recorded", not exit 0.
- **13A-5b-iii-b split in two (13A-5b-iii-b-1).** The unstaged fill
  reporting and the `work_units` preparation terms with per-stage ranges must
  land TOGETHER (13A-5b-iii-a's decision above), and their re-basing alone —
  the four suites' entries and bench probes, the seven non-guide byte cancel
  tests and every prep-bearing byte `work_units` / progress pin (a cascade of
  ≈12 test files) — exceeds one run; the policy lands first (13A-5b-iii-b-1),
  the atomic progress change is 13A-5b-iii-b-2.
- **Byte preparation reports carry stage 1 (13A-5b-iii-b-2).** The fill's
  chunks and every range's advance report stage 1 ("preparing") and inner
  blurs stage 6, so each preparation stage keeps an observable entry — its
  first stage-6 report — even though a reporting fill and ranged stages move
  progress off 0 under `fetch_max`; the typed path keeps stage 6 (stated).
- **`past_the_fill` counts two polls per chunk (13A-5b-iii-b-2):** `every`
  once, then the advance's `report`, which checks.
- **The materialization term's shape (13A-5b-iii-b-2):** `|rect|·(1 + rows
  inside)` — the buffer and one pass per row the chain composites — plus the
  subtree's mask/plane work and the group's own mask blur, all spent inside
  `rendered_group`; the range is flat at its start (the chain reports
  nothing), tighter per-child figures are 13B's.
- **The materialized arm is dispatched before the mask derivation
  (13A-5b-iii-b-2).** `Node::resolve` derived every contribution's mask
  first and the materialized arm discarded it (`rendered_group` re-derives
  the same cached plane); dispatching first moves that blur inside the
  materialization range and changes no byte.
- **`float_work` is infallible (13A-5b-iii-b-2),** so a work term can never
  add a refusal: `float_plan`'s `pixel_count` refuses the factor-rounded
  extent, which `padded_dimensions` does not bound; the byte plan admits
  exactly what it admitted.
- **13A-5b-iii-b split again: the blocking seam is 13A-5b-iii-b-3
  (13A-5b-iii-b-2).** DECIDED: `#[doc(hidden)] RzJobControl::park_at(count)`
  / `release()`, `checkpoint()` parking on a `Condvar` at that count, the
  four suites' default count-triggered tests taking their entry count from
  the worker's own uncancelled state timeline — which removes the
  "returned before the entry" residual. It touches `job_control.rs`'s hot
  checkpoint and every default cancel test, so it lands on its own after the
  progress change.
- **The entry count is the worker's own recorded report log, not a polled
  timeline (13A-5b-iii-b-3),** because a polled calibration carries the same
  stall the parked cancel removes: a poller can miss the entry report in the
  calibration exactly as it missed it in the cancel. The log is written by
  the worker inside `RzJobControl::report`, so it holds every report.
- **The parked state is the verdict's state (13A-5b-iii-b-3).** The verdict
  (progress, stage) and `checkpoints == count` are read while the worker is
  parked, not at its return, so the verdict names the checkpoint the cancel
  was observed at exactly.
- **A never-reached park count is a calibration failure and must surface
  immediately (13A-5b-iii-b-3):** the worker releases on return and every
  parked assertion sits under a cancel-and-release guard (`Released`), so a
  bad count or a failed assertion fails in a wake, never a 240 s wait or a
  hung join. Declined: a library-side guard type, a `released()` getter and a
  pre-flight "count ≤ last checkpoint" check — the immediate "returned first"
  failure already names the miscalibration, and the seam's surface stays six
  methods and `Report`.

- **13A-5c split in three (13A-5c-1).** Its three owned items are
  independent and each is a run: the description preparation touches the
  byte admission and walk and adds one scratch term that no existing pin
  sees (13A-5c-1); the `inpaint_source.rs` fallback is a retouch sampling
  path with its own tiling, oracle and suites (13A-5c-2); the three
  "(resource accounting)" items re-base the byte plan's scratch pins across
  the regional suites, as does charging the walk's `from_meta` transient on
  a modelled profile, so they land together (13A-5c-3). 13A-5c-1 goes first:
  it is the smallest and the other two read its admission order.
- **The typed path parses no description (13A-5c-1).**
  `PrecisionAdjustment::parse` binds `prepare_precision(profile)?` at EDIT
  time (`adjust_precision.rs:73`), so a typed render never parses a layer
  description: `precision_regional.rs` and the typed plan are unchanged.
- **The context term is profile-gated, the cutout's rule (13A-5c-1).** Only
  an unmodelled profile makes `validate_context` parse, so only there is the
  parse charged; the walk's own `from_meta` transient under a modelled
  profile (memoized above 64 KiB) stays uncharged, because charging it
  re-bases every byte plan pin — that move belongs with 13A-5c-3's re-base.
- **The walk polls once per node, not inside a parse (13A-5c-1).** A serde
  parse cannot be polled; the interval is ONE parse, and on the unmodelled
  path the walk's `from_meta` of a meta above 64 KiB is a memo hit (the
  admission parsed it).
- **13A-5c-2 split in two (13A-5c-2a).** Masks and pointwise/opaque
  metadata tile with no change to the compositor (the aligned plane is
  layer-anchored and cached; a pointwise adjustment reads one sample; opaque
  meta is never interpreted), and the fallback becomes cancellable by running
  the chain 13A-5b-ii-a/b already polls under the sampling job — one run.
  Groups and styles need a per-tile D5 / Shallow snapshot origin rule,
  styled-group materialization over the full extent and per-tile
  `RenderBudget` semantics, or the byte regional compositor given a `Cut`:
  a design decision of its own (13A-5c-2b).
- **The plan predicate parses nothing (13A-5c-2a).** `promote_flattened_plan`
  is a plan, so `pointwise(doc)` answers full-frame for ANY metadata; the
  render may tile where the plan predicted full-frame, which only
  over-reserves, and the plan adds `tile_scratch_bytes` and `context_scratch`
  so it bounds both arms whichever it predicted.
- **The plate still reports no progress (13A-5c-2a).** Every caller
  (`rz_doc_retouch_source`, `doc_inpaint`, `doc_content_aware` ×2, the
  promotion) runs it as a preparation whose work its own progress does not
  count, so both envs take `checkpoints_only()`; a per-tile / per-row report
  is 13B's.

- **The tree, not the tile loop (13A-5c-2b).** Tiling
  `composite_level_into` would need a per-tile D5 / Shallow snapshot origin
  rule, full-extent materialization of nested styled groups (which
  `rendered_group` clips to the accumulator) and index-rewriting
  substitution — a second compositor. The regional tree already encodes
  every one of those rules and is proven sample-for-sample against
  `rz_doc_flattened`; given a `Cut` it gates contribution exactly as the
  reference's `env.cut` does. Its cost is the SAMPLE-MAJOR cancel unit,
  declared as `c_tree_row` and measured, not gated.
- **Guided plates stay full-frame (13A-5c-2b).** A guide-reading
  Shadows/Highlights is the tree's one STAGED operator; per-tile stages
  would re-run each guide over halo-grown tiles. The fallback keeps it
  (owner 13B).
- **The arm is chosen in one pass (13A-5c-2b).** `plate_arm` parses each
  contributing entry's metadata once, polling before each parse, and
  decides Tiles / Tree / FullFrame from the same parse (carried nit 1).
- **The plan pays for the tree's tile on small canvases (13A-5c-2b).** The
  promotion's covering invariant holds term by term except `20·tile` against
  the accumulator's 16n; the non-cooperative arm adds `(20·tile − 16n)⁺`
  rather than a row-high tree accumulator, which would only move the corner
  to H = 1.

- **The parse is charged under every profile (13A-5c-3).** The walk's
  `from_meta` runs whatever the profile, so the profile gate that is right
  for the preflight alone (`context_scratch`) under-reserves the regional
  admission. `parse_scratch` is the same figure ungated; the other
  `context_scratch` callers keep it (the Tiles/FullFrame plate arms'
  re-base would move the conversion suites — 13B).
- **The uncharged nest term is a peak mirror, not a charge (13A-5c-3).** It
  is scratch the plan declares and the render reserves; charging it to the
  `RenderBudget` would change the reference's own refusal sentences.
  Conservative where exactness would need the reference's run-time geometry
  (the affected window's width, a borrowed crop), never short.
- **D5 / Shallow snapshots stay a stated difference, with cause
  (13A-5c-3).** The budget bounds memory a nest holds; the regional path
  holds neither snapshot (per-sample one-element backdrops; where the staged
  path does hold `S` or a pass-through hold over `R_0`, `stage_scratch`
  charges it). A dry-run charge for an allocation that never happens would
  refuse renders that fit — the opposite of 13A's purpose — so regional
  admits a superset by design; `the_background_and_pass_through_snapshot_
  charges_are_a_stated_difference` stays the pin.
- **A zero-opacity member of an unmaterialized unit is folded, not collected
  (13A-5c-3).** It is one more isolated group the regional path never
  materializes, no different from an opacity-1 member sampled per sample;
  inside a materialized nest `unit_charges` already pays it because the
  reference renders that nest.
- **The Shallow plane rides through a pass-through level (13A-5c-3).** A
  pass-through level's children see a Shallow plane when THEIR level wants
  one or when an enclosing pass-through level's did (the reference hands
  `env` through unchanged otherwise), so the dry run threads `shallow ||
  level_wants_shallow(children)`; a nest root is `KnockoutBackdrop::NONE`.
- **The typed materialization terms stay sums (13A-5c-3).** They over-reserve
  (sums, not peaks; a styled raster charged twice) — 13B's tighter plan
  figures.
- **13A-6 is split by coverage kind (13A-6a).** Selection, artwork and long
  strokes share nothing but the "no cap raise, no resolution cut" rule: each
  needs its own bounded evaluation (support windows, an active-edge sweep, a
  footprint field) against its own retained reference, so each is its own run
  (13A-6a / 13A-6b / 13A-6c).
- **Bound by the SUPPORT, not by tiles (13A-6a).** Every spatial selection
  kernel's output is exactly zero beyond its input's support grown by its
  reach, so one window per stage is exact and costs one crop and one embed;
  tiling would re-run the blur's halo per tile for no gain on the small
  selections the cap refused.
- **The work check moves to `apply` (13A-6a).** A support is only known with
  the samples in hand; `plan` has dimensions only. The sentence is unchanged,
  so a full-support refusal surfaces from compute instead of plan, which the
  host already handles identically.
- **Refine keeps a SUMMED check (13A-6a).** Charging each stage separately at
  apply would let a per-stage-under / sum-over refinement run up to ~3× the
  cap; the sum is charged at the input support grown by the cumulative reach,
  an upper bound on every stage's own window, and reduces to today's sum on a
  full support.
- **The EDT envelope moves to f64, once, for every consumer (13A-6a).** A
  cropped morphology equals the whole plane only if the 1-D envelope is
  translation-exact, which f32 is not past coordinate ≈4096. `edt_1d` is the
  ONE envelope (byte, typed, inpaint, matting), so it changes in place — no f32
  twin is retained; `d`, `sqrt` and the byte ramp stay f32, so every result
  below that regime is unchanged.
- **The whole-plane path stays as a hidden oracle (13A-6a).** The bounded path
  is proven against the pre-13A-6a arithmetic, not a restatement:
  `reference_apply` runs the same `spatial_kernel` / `compose` /
  `contrast_curve` over the canvas, so the oracle and the product cannot drift.
- **Bound by the outline's rows, not tiles (13A-6b).** Coverage is exactly
  zero on every row no edge overlaps, and a row's work is its active edges;
  an active-edge sweep visits only the rows between the outline's top and
  bottom and reads only the edges each row holds, so tiling (which would
  re-select edges per tile) buys nothing.
- **The row body is shared, the selection differs (13A-6b).** `row_coverage`
  is the pre-13A-6b loop body verbatim; the sweep and `reference_rasterize`
  differ ONLY in how they fill `active` (a sorted cursor + `retain` versus
  every edge in outline order), and the order is proven not to reach a
  sample — so the oracle and the product cannot drift.
- **Charge `n` up front, never `n + E`: the sweep is bounded by the reference
  at every checkpoint (13A-6b).** With `n` alone, `Σ active ≤ E·H` makes the
  sweep's running total ≤ the reference's at every `charge`, so nothing the
  full scan accepts is refused; `+ E` would refuse an outline the full scan
  accepts at exactly the cap (pinned; negative control B). The in-place sort
  is uncharged, bounded by `MAX_PATH_EDGES`.
- **The 64M render aggregate stays: extent-bounded input, windowed components
  are 13D's (13A-6b).** `rz_artwork_render` is a pointwise pass over planes
  the host already built at the artwork's own extent; the aggregate bounds
  that input, which regional evaluation cannot shrink. Only a per-component
  window in the contract could, and that is a new interface.
- **Bound by the stroke's footprint, classify the hard edge exactly
  (13A-6c-1).** Coverage is exactly zero outside the union of the dab boxes
  and exactly 1.0 / 0.0 inside / outside a hard dab's margin-shrunk /
  margin-grown ellipse, so a footprint field plus an interior/exterior
  classifier reproduces the canvas-field path bit for bit while the 8×8 sum
  runs only on the edge band (O(perimeter) per dab, not O(area)). The
  canvas-field path stays as the hidden oracle `reference_rasterize`, sharing
  the ONE per-pixel `evaluate`, so the oracle and the product cannot drift.
- **EXTERIOR needs the centre outside the pixel (13A-6c-1, objection 1).**
  An edge-distance test alone misclassifies a pixel whose parallelogram
  surrounds the origin (a sub-pixel dab, or a ≥ 1-px needle inside one
  pixel): its edges are all far from the origin while samples inside are
  covered. EXTERIOR therefore also requires the dab centre outside the closed
  pixel square — exact, comparisons only, and by the affine bijection exactly
  "the origin is outside the parallelogram". The margin claim is scoped to
  its domain: the classifier runs only for `d[2] ≥ 1` (minor radius ≥ 0.005),
  maps corners with `sample`'s own expressions, and sub-1-px hard dabs
  integrate their ≤ 2×2 box. Two named regression cases pin it.
- **The WHOLE-STROKE dab model stays (13A-6c-1).** One quantization after
  the complete stroke is RZBR v1's contract; a per-batch model would quantize
  per batch and change recorded replays. The 50,000-dab cap and
  `raw_local_coverage`'s 4,096 are 13D's.
- **Never charged above the old plan (13A-6c-1).** `n + |F|` up front and
  `min(64·box, box + 64·band_bound)` per hard dab are each at most the old
  `2n` and `64·box`, so every prefix total is at most the old one and nothing
  the old plan accepted is refused; the scratch `8·|F| + 2n` is at most `12n`.
- **The route proof is a refusal, from one build (13A-6c-1, objection 2).**
  B1 alone could pass on a path that never reached the changed rasterizer, so
  B4 drives a stroke whose NEW charge exceeds the cap (≥ 531 fully-on-canvas
  512-px dabs, Σ ≥ 294M) and requires the core's own sentence and an
  untouched document. The `stroke_version: 1` route does reach
  `rz_brush_u16_plan` / `rz_brush_u16`; the ×257 adapter serves only
  UNVERSIONED point strokes (13A-6c-2's). A pre-change-build run was declined.
- **One walk, not a second generator (13A-6c-2).** The unversioned 16-bit
  typed route calls `NativeBrushStrokeSession.generated` — the generation walk
  MOVED verbatim out of `nativeStrokeArguments` — with unit pressure, zero
  times and no flags, so an unversioned call and its `stroke_version: 1` twin
  deposit the same dabs (T1 asserts exact equality) and a recorded unversioned
  step replays through the same deterministic walk. The recording path and
  `ActionSteps.swift` are untouched.
- **The work cap is a contract delta, stated not avoided (13A-6c-2,
  objection 1).** Routing unversioned 16-bit strokes through `rz_brush_u16`
  subjects them to `MAX_BRUSH_WORK` as well as the dab cap; the catalog,
  README, `app/CLAUDE.md` and the contract deltas name both limits, and T4 /
  T4b pin the core's verbatim sentence on a ≈801-dab stroke far inside the dab
  cap. 8-bit unversioned strokes are untouched.
- **Never COMMIT a prefix; the canvas preview stops growing (13A-6c-2,
  objection 2).** A refusing dab never reaches the overlay (`stampSoftDab` /
  `stampCloneDab` return after the session's refusal, mirroring
  `stampNativePixelAirbrush`) and `renderStroke`'s per-iteration check stops
  the walk, so the preview freezes at the last admitted dab and mouse-up
  presents the refusal; nothing is committed. Clearing the overlay mid-stroke
  would exceed the frozen-file budget and is out of scope.

## Verification record (2026-09-23)

| Check | Result |
|---|---|
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_document` | 13 passed |
| `cargo test --release --test regional_precision` | 10 passed |
| `cargo test --release --test integration` (NULL sweep incl. the four exports) | 25 passed |
| `make lint` | clean |
| `make test` after the extraction alone | 2222 passed, 0 failed |
| `make test` final | 2245 passed, 0 failed (239 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a1_regional.py` | ALL GATES PASS (receipt `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-1/20260923-105038/report.json`) |

Rust coverage: all 27 blend modes on a two-layer stack (8 bits and u16); offsets
partly and wholly off-canvas including `(i32::MIN, i32::MAX)`; hidden and
zero-opacity skips (a hidden styled+masked layer and a zero-opacity styled one
are admitted); six pointwise adjustment ops — invert, levels, hue_saturation,
`document_v2` exposure, posterize, zero-amount shadows_highlights — under
Normal, Multiply and Dissolve; a `linear_light` document at 8 and 16 bits; the
four edges, corners, 1×1, empty and overflowing requests; divisors
{1, 2, 3, 7, u32::MAX}; the exact output-budget boundary; every refusal
construct (group, hidden child, nested, empty group, clipped, style, Blend If,
enabled mask, guided Shadows/Highlights) with a DISABLED mask admitted; the
hidden-clipped-member case at 8 bits and u16 (and its unclipped twin admitted
and equal); the 11E refusal equal between plan, render and the full render at
8 bits and u16; plan JSON fields; pre-cancelled control; a concurrently
cancelled 3000×2000×7-layer render returning "operation cancelled" (a
load-sensitive timing test of the same class as the known cancellation flakes);
admission refusal at a 1000-byte working limit; the float fixture plus
`add_layer` and an Exposure adjustment, bit for bit; NULL sweeps.

Drive gates (fixed before measuring, EXACT): R1 8 bits and R2 16 bits — interior,
top-left edge, bottom-right edge, 1×1 and divisor-3 preview equal the full
render's crop; R3 the float fixture + `new_layer`; R4 `add_layer_mask` refuses
by name with the layer index at 8 and 16 bits; R5 an out-of-canvas region,
divisor 0 and `region` beside `layer`/`channel` refuse in band; `list_history`
is unchanged by every regional call. **Fixture note:** at 8 bits
`add_adjustment_layer` always adds a reveal-all mask, which 13A-1 refuses while
enabled — the receipt records "regional render refuses layer 3 (Hue): carries
an enabled layer mask" — so the drive DISABLES that mask before gating R1 (a
disabled mask is inside the admitted set). The 16-bit adjustment layer is
created without a mask.

## 13A-2a verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the `effective_mask_rect` hoist and the two visibility changes, before any regional change | 2245 passed, 0 failed (239 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_masks_clips` (new, 8 bits) | 17 passed |
| `cargo test --release --test regional_precision_masks_clips` (new, u16 + float) | 12 passed |
| `cargo test --release --test regional_document` / `regional_precision` | 13 / 10 passed (the clip, mask and Blend If refusal assertions are now admission-equals-reference assertions; plan rows carry `clip_base`/`mask`/`blend_if`) |
| `cargo test --release --test integration` | 25 passed |
| `make lint` | clean |
| `make test` final | 2274 passed, 0 failed (241 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a2_masks_clips.py` | ALL GATES PASS (receipt `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/13a2a/report.json`) |
| `scripts/verify_phase13a1_regional.py` (auto mask left enabled; R4 now asserts the masked render EQUALS the crop) | ALL GATES PASS (receipt `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/13a1/report.json`) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/12e2b_placement/receipt.json`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS, exit 0 (these drives print no summary line; `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/12h_conversion/report.json`, `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/12g_clipboard/report.json`) |
| `scripts/verify_phase9h_ui.py --require-current` | passed (56 groups + pass 2: 1; `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2a/20260923-115350/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description and the typed `render` note); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files, independent oracle = the full render cropped
and top-left-cell sampled, windows at every canvas edge and corner, 1×1, the
whole canvas and four windows straddling each mask's corners, divisors
{1, 2, 3, 7, u32::MAX}): reveal-all, hide-all and gradient masks; a mask
offset apart from its layer (partly outside) at outside coverage 0 and 255;
density 0.5; feather 6 and feather 40 (both blur schedules) at outside 0/255
and density 1/0.5; a disabled mask; a masked adjustment (canvas-sized, smaller
than the canvas and partly off-canvas pixel extents, feathered and not, Normal,
Multiply and Dissolve); clip units — a Multiply base with a raster Multiply 0.6
member and an adjustment member, a hidden member, a zero-opacity member, a
hidden base (with a styled member, admitted unchecked), a zero-opacity base, an
adjustment base with a visible member (contributes nothing at both depths), an
Invert base whose ONLY member is hidden (8 bits: nothing, u16: applies; each
equal to its own reference, the plan listing the base with no member rows),
baseless clipped entries at indices 0 and 1 (both `clip_base: null`, entry 1
visible beyond entry 0's footprint), a Dissolve base and member, a masked base,
masked members; typed Blend If on a lone raster, a clip base (with and without
a mask) and a member; a `linear_light` document at 8 and 16 bits (Blend If is
refused by the 11E policy under linear light, so that case carries none); the
float fixture + `add_layer` + `set_mask` (feather 3, density 0.8) + `layer_clipped`
bit for bit; plan fields (`clip_base`, `mask`, `blend_if`, spatial halo,
`required_input_region` clamped at a canvas edge, scratch/work formulas);
refusals still named for a styled member, byte Blend If ("carries a layer
style"), a group member (its child: "is inside a group") and a hidden empty
group member ("is a group"); a pre-cancelled control at both depths.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples (one checkpoint). MEASURED by
`cd core && cargo test --release --test regional_masks_clips a_concurrently -- --nocapture`
on a 3000×2000 document of a background unit plus one masked clip unit (base +
two members, unfeathered masks at density 0.5): cancel-to-return 0.031 ms and
0.008 ms on two runs, against 37.9–38.8 ns per unit-sample measured from the
uncancelled render of the same document (455–466 ms) — ≈811 and ≈212 samples
after the cancel, inside the declared 4096. The test asserts the result
("operation cancelled", no image, zero scratch) and prints the measurement; it
is load-sensitive, of the same class as the known cancellation flakes.

Drive gates (fixed before measuring, EXACT): M1 8 bits — the 13A-1 stack with
`add_adjustment_layer`'s automatic reveal-all mask ENABLED equals the crop,
then `select_ellipse` (feather 6) + `add_layer_mask from_selection` +
`set_mask_properties` (density 0.5, feather 8) equals the crop; M2 the same at
16 bits; C1/C2 `set_layer_clipped` over a Multiply 0.6 base with a visible and a
hidden member and index 0 clipped (baseless) at 8 and 16 bits; C3 an Invert base
whose only member is hidden — the 8-bit full render is unchanged by the unit,
the 16-bit one changes, each regional render equals its own crop; F1 the float
fixture + `new_layer` + `add_layer_mask` + `set_layer_clipped`; B1 16-bit
`set_layer_style` with `blend_if` only equals the crop; B2 8-bit `set_layer_style`
with `blend_if` only refuses "regional render refuses layer 1 (Top): carries a
layer style" (13A-2b); N1 `{"region": 5, "layer": 0}` answers "region and
preview_divisor apply to the flattened canvas only"; `list_history` unchanged
by every regional call.

## 13A-2b verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the `composite_pixels_sample` / `Shape::coverage_byte` extraction, before any regional change | 2274 passed, 0 failed (241 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_byte_styles` (new, 8 bits) | 10 passed |
| `cargo test --release --test regional_masks_clips` | 17 passed (byte Blend If refusal swapped for a knockout refusal) |
| `cargo test --release --test regional_precision_masks_clips` | 13 passed (nit 1 exact formulas, nit 2 u16 concurrent cancel) |
| `cargo test --release --test regional_document` / `regional_precision` / `integration` | 13 / 10 / 25 passed |
| `make lint` | clean |
| `make test` final | 2285 passed, 0 failed (242 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a2b_byte_styles.py` | ALL GATES PASS (receipt `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/13a2b/report.json`) |
| `scripts/verify_phase13a2_masks_clips.py` (B2 now asserts 8-bit Blend If EQUALS the crop) | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/13a2a/report.json`) |
| `scripts/verify_phase13a1_regional.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/13a1/report.json`) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/12e2b_placement/receipt.json`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/12h_conversion/report.json`, `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/12g_clipboard/report.json`) |
| `scripts/verify_phase9h_ui.py --require-current` | passed (56 groups + pass 2: 1; `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-2b/20260923-125258/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (`regional_byte_styles.rs`, independent oracle = `rz_doc_flattened`
cropped and top-left-cell sampled, the 13A-2a windows and divisors
{1, 2, 3, 7, u32::MAX}): a lone raster with fill 0.5, fill 0, a gray Blend If on
both ramps, a red `underlying`-only ramp and fill + Blend If, each under
Normal/Multiply/Dissolve at opacity 0.9 and 1.0 over translucent pixels, and
each with a feather-6 mask; a disabled effect (with fill 0.5) on a lone raster
and a clip base; a styled clip base (fill 0.5 + Blend If; fill 0, whose members
show inside its shape; a Dissolve base; masked at feather 0 and 6;
`blend_interior_as_group`) under a member that exceeds the base rect; a styled
member (fill 0.4 + Blend If) under an unstyled and a styled base; the clamp-only
step (the 51/52 fixture, the step before and after a contributing member, the
member hidden; plan rows `[0, 1]` / `[0, 1, 3]` / `[0, 1, 2]` and
`work_units == pixels·(rows + 1)`); refusals (`drop_shadow` → "carries a layer
style", knockout → "carries a knockout" on a lone raster and a member, an
effect named before a knockout, the knockout member hidden or at opacity 0 →
admitted); plan rows (`blend_if` true only on a styled Blend If row, scratch
`pixels·16 + pixels·4`, work `pixels·(rows + 1)`); `with_evaluation(LinearLight)`
on a styled document is `Err`; a pre-cancelled control.

**Measured cancellation bound (u16 twin).** DECLARED before measuring: a cancel
returns within 4096 samples. MEASURED by
`cd core && cargo test --release --test regional_precision_masks_clips a_concurrently -- --nocapture`
on a 3000×2000 document of a background unit plus one masked clip unit (base +
two SoftLight 0.9 members, unfeathered masks at density 0.5 on records 1 and
2): cancel-to-return 0.042 ms and 0.011 ms on two runs, against 45.0–45.5 ns per
unit-sample from the uncancelled render (540–546 ms) — ≈928 and ≈253 samples
after the cancel, inside the declared 4096. Load-sensitive, of the same class
as the known cancellation flakes.

Drive gates (fixed before measuring, EXACT): S1 a lone `place_image` raster +
`set_layer_style {fill_opacity: 0.5, effects: []}` equals the crop, then
`{fill_opacity: 0.5, blend_if, effects: []}`; S2 Blend If only; S3 a Multiply
0.6 base styled `{fill_opacity: 0.3, blend_if}` with a visible, a hidden and a
visible opacity-0 clipped member, then the base at fill 0, then the visible
member styled `{fill_opacity: 0.5}` — each equals the crop; S4
`{effects: [drop_shadow]}` → "regional render refuses layer 1 (Top): carries a
layer style" and `{fill_opacity: 0.5, knockout: "shallow"}` → "regional render
refuses layer 1 (Top): carries a knockout", `list_history` unchanged; every
regional call leaves `list_history` unchanged.

## 13A-3a verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the four extractions (`quantize_sample`, `encode_sample`, `composite_group_sample16`, `gate_pass_through_sample16`), before any regional change | 2285 passed, 0 failed (242 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_groups` (new, 8 bits + both concurrent cancels) | 17 passed |
| `cargo test --release --test regional_precision_groups` (new, u16 + float) | 12 passed |
| `cargo test --release --test regional_document --test regional_precision --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test integration` | 13 / 10 / 17 / 13 / 10 / 25 passed (the group refusals are now admission-equals-reference assertions) |
| `make lint` | clean |
| `make test` final | 2314 passed, 0 failed (244 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3_groups.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3a/20260923-140845/verify_phase13a3_groups/report.json`) |
| `scripts/verify_phase13a2b_byte_styles.py` (nit 2) / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` | ALL GATES PASS each (same stamp, one folder per drive) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (same stamp) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 55 groups PASS; `current-layer-commands-ui` FAILED on activation ("the owned editor never became the real key window") with the console LOCKED (`IOConsoleLocked` true, HID idle ≈23 min) — re-run alone with `RZ_PAINT_UI_CASES=current-layer-commands-ui`, same result: the known screen-lock case, not a code failure (`…/20260923-140845/ui-probe/`, `…/ui-probe-current-layer/`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description and the typed `render` note); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files, independent oracle = the full render cropped
and top-left-cell sampled, the 13A-2 windows and divisors {1, 2, 3, 7,
u32::MAX}), at 8 bits AND at u16: a pass-through group plain, at opacity 0.5,
with a canvas mask (density 0.5, feather 6 — the group row declares the
halo), with both; an isolated group in Normal / Multiply / Dissolve at opacity
1 and 0.7, masked (feather 0 and 6), with a hue/saturation child that reaches
only the group's own pixels (asserted against the background outside both
rasters) and a Levels child, with a masked child; a typed isolated group with
Blend If (lone and masked); groups nested three deep mixing kinds (Multiply
isolated ⊂ gated pass-through ⊂ masked isolated) with the full plan rows
(`index`, `group`, `kind`, `operator`, `clip_base`, `group_depth` 3, scratch and
work formulas) and a hidden middle group; ten nested groups at 8 bits (the
depth cap: an eleventh grouping is refused by `group_layers`); a group as clip
base with a visible member (isolated), a Multiply 0.7 group base with a masked
member, a pass-through GROUP member (isolated Normal), the members hidden
(pass-through again, plan rows shrink); the objection's hidden-only fixture at
opacity 1 and 0.5; a baseless clipped group at the root and at the bottom of a
group's level; a hidden (and a zero-opacity) group holding a drop-shadow child
admitted unchecked, visible it refuses by the child's index; empty groups
isolated, pass-through, as a clip base (contributes nothing) and as a clip
member; the empty-extent member drop under a styled base (52 / 52 / 51); 8-bit
effect-less styled groups (fill 0.5 + Blend If, fill 0, masked, as a clip base
with a member and with a clamp-only member); a `linear_light` document with a
nested Multiply group at 8 and 16 bits; typed plan scratch Σ over two
feathered masks (group + child); refusals — an 8-bit group with a drop shadow
and a 16-bit group with a fill-only style by the GROUP's index, a knockout
child by its own index, a guided Shadows/Highlights inside a group by its own
index, the fixed order (a styled member named before the base group's knockout
child), the reference's nesting-limit sentence; pre-cancelled controls; the
float fixture + `add_layer` + `group_layers` (minted isolated), Multiply,
masked (feather 3, density 0.8) and nested, bit for bit.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 ROOT samples. MEASURED by
`cd core && cargo test --release --test regional_groups a_concurrently -- --nocapture`
on 3000×2000 documents of a background plus three SoftLight 0.9 rasters, two of
them inside a Multiply isolated group nested in a gated (0.9) pass-through group
(the u16 twin the same shape, typed; the two tests run concurrently): 8 bits
cancel-to-return 0.071 ms and 0.349 ms on two runs against 133.7 / 121.4 ns per
root sample (uncancelled 802 / 729 ms) — ≈534 and ≈2874 root samples after
the cancel; u16 0.005 ms and 0.005 ms against 138.4 / 123.2 ns per root sample
(uncancelled 831 / 739 ms) — ≈33 and ≈43 root samples. All inside the declared
4096. Load-sensitive, of the same class as the known cancellation flakes.

Drive gates (fixed before measuring, EXACT): G1 8 bits / G2 16 bits —
`group_layers` (Pass Through) over the 13A-2 clip stack (Multiply 0.6 base, a
visible and a hidden member, index 0 clipped and baseless, a hue/saturation
adjustment inside the group) equals the crop, then `set_layer_properties`
opacity 0.6, then `add_layer_mask` from an ellipse selection (feather 6) +
`set_mask_properties` density 0.5, then `blend_mode: "Multiply"`, then a nested
outer group; G3 8 and 16 bits — a pass-through group (raster + Invert) as clip
base with a visible member (`set_layer_clipped`) and a group member, then both
members hidden (`set_layer_properties` visible false), the base Pass Through
again: equals the crop and the full render returns to the pre-member one; G4
the float fixture + `new_layer` + `group_layers`; G5 8 bits `set_layer_style`
`drop_shadow` on the group → "regional render refuses layer N (Group): carries
a layer style", 16 bits `{fill_opacity: 0.5}` on the (Normal) group likewise;
`list_history` unchanged by every regional call. **Drive note:** a 16-bit
`get_document` row reports a group's mode as a numeric code and its record kind
(`pass_through_group`), so the drive's Pass Through check accepts either form.

## 13A-3b-i verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the extractions (`composite_contribution_sample`, `composite_pixels_sample16` / `PixelPass16`, `typed_coverage_byte`, `alpha16` visibility), before any regional change | 2314 passed, 0 failed (244 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_byte_effects --test regional_precision_styles` (new) | 16 / 12 passed |
| `cargo test --release --test regional_groups --test regional_byte_styles --test regional_precision --test regional_precision_groups --test regional_document --test regional_precision_masks_clips --test regional_masks_clips --test regional_render --test integration` | 17 / 10 / 10 / 12 / 13 / 13 / 17 / 5 / 25 passed (the style refusals are now admission-equals-reference assertions or the new sentences) |
| `make lint` | clean |
| `make test` final | 2342 passed, 0 failed (246 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3b_styles.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3b-i/20260923-153005/verify_phase13a3b_styles/report.json`) |
| `scripts/verify_phase13a2b_byte_styles.py` (S4 effect admitted) / `verify_phase13a3_groups.py` (G5 new sentence, 16-bit fill-only group equals the crop) / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` | ALL GATES PASS each (same stamp, one folder per drive) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (same stamp) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | passed (56 groups + pass 2: 1; console unlocked; `…/20260923-153005/ui-probe/`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description and the typed `render` note); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files, independent oracle = the full render cropped
and top-left-cell sampled, the 13A-2 windows plus windows straddling the
styled rect's grown edges, divisors {1, 2, 3, 7, u32::MAX}), at 8 bits AND
u16: drop shadow (distance 3 / size 2, and distance 9 crossing the window and
canvas edges), outer glow, stroke outside / inside / center, a Multiply colour
overlay, gradient overlay aligned and canvas-aligned (radial, Overlay), inner
shadow at distance 0 and 40 (size 0), inner glow, a two-part bevel, satin —
each at opacity 1, 0.6 Normal and Multiply (the package) and Dissolve 0.6 (the
direct path), on a shaped (elliptical, holed, graded-alpha) layer near the
canvas corner; a partly off-canvas styled raster; fill 0.5 + Blend If +
effects; a masked styled raster at feather 0 and 6; a styled clip base with a
visible member exceeding its rect and a visible zero-opacity (clamp-only)
member, in all four variants, and with the clamp-only step alone; a fill +
Blend If + effects base; a styled member (opacity 1 and 0.6) under an
unstyled and a styled base; a styled raster inside a pass-through group
(plain and gated), an isolated group (Multiply, Normal 0.8) and nested; 8-bit
global light (`set_global_light` 30°/55° with `use_global_light`, the full
render differing from the default light); u16 typed Blend If beside a style,
lone and as a base; u16 effect-less styled groups (fill 0.5, fill 0, fill +
Blend If; Normal and Multiply 0.7) lone, masked, as a clip base with a member
and as a member; plan rows (`style`, the Global row, `global_analysis`
`["layer_style_planes"]`, `required_input_region` the whole canvas for a small
window in the middle of a large layer, a feathered mask on the styled layer
adding no halo while a feathered unstyled layer beside it sets the top-level
neighbourhood; `"fill"` rows — a disabled effect, and a 30000 × 1073 layer
whose inner shadow's padded plane passes `MAX_PIXELS` — keeping
`global_analysis` false and the grown window); scratch and work restating
`style_scratch` (outside stroke 3 + colour overlay: pad 5, 3 taps); the reach
tests; refusals in order (knockout, grouped interior, knockout before grouped
interior, a group with effects before its knockout, a base's refusal before
its member's, a hidden styled group admitted unchecked, the flag without a
grouped interior effect admitted, a fill-only group admitted); pre-cancelled
controls; the float policy assertion. Mutation-checked: replacing the package
mix by the copy fails 9 byte tests; dropping the typed clip clamp fails 4
typed tests.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 ROOT samples once the loop has begun (plane rendering before the
loop is the reference's own uncancellable preparation). MEASURED by
`cd core && cargo test --release --test regional_byte_effects a_concurrently -- --nocapture`
on 3000×2000 documents of a background plus a 2960×1960 SoftLight 0.9 raster
(every seventh pixel transparent) styled with a drop shadow and a Multiply
colour overlay — the package path (the u16 twin the same shape; the two tests
run concurrently). The probe waits until the progress passes the pre-loop
fraction, then cancels; the per-sample figure is the uncancelled LOOP time
(from the same probe point to the return) over the canvas. 8 bits:
cancel-to-return 0.057 ms and 0.015 ms on two runs against 78.15 / 85.74 ns
per root sample (loop 468.9 / 514.4 ms) — ≈728 and ≈177 root samples after the
cancel; u16: 0.056 ms and 0.037 ms against 86.32 / 93.84 ns (loop 517.9 /
563.0 ms) — ≈653 and ≈400 root samples. All inside the declared 4096.
Load-sensitive, of the same class as the known cancellation flakes.

Drive gates (fixed before measuring, EXACT): E1 8 and 16 bits — a lone
`place_image` raster + `set_layer_style {drop_shadow, stroke, color_overlay}`
equals the crop, then opacity 0.6, then Multiply; E2 8 and 16 bits — a styled
clip base `{effects, fill_opacity 0.3, blend_if}` with a visible clipped
member equals the crop; E3 16 bits — a fill-only styled Normal group equals the
crop; E4 — `{knockout: "shallow"}` "carries a knockout", `{blend_interior_as_group:
true, effects: [color_overlay]}` "blends interior effects as a group", an 8-bit
group + drop shadow "is a group with layer-style effects", each naming the
layer; `list_history` unchanged by every regional call. **Drive note:** MCP
`render` returns the image, never the plan (`RasterCore.renderRegionPlan` has
no MCP caller), so E1's `global_analysis == ["layer_style_planes"]` /
`required_input_region` and E3's `global_analysis == false` are asserted by
the Rust tests, not the drive.

## 13A-3b-ii verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the three nits (the split, the rewrap, the const), before any behaviour change | 2342 passed, 0 failed (246 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_byte_styled_groups --test regional_precision_styled_groups` (new) | 22 / 13 passed |
| `cargo test --release --test regional_byte_effects --test regional_precision_styles --test regional_groups --test regional_precision_groups --test regional_document --test regional_precision --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test regional_render --test render_failure_tests --test integration` | 16 / 12 / 17 / 12 / 13 / 10 / 17 / 13 / 10 / 5 / 6 / 25 passed (the four group-effects refusals are now admission-equals-reference, or "carries a knockout" naming the group) |
| `make lint` | clean |
| `make test` final | 2377 passed, 0 failed (248 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3bii_styled_groups.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3b-ii/20260923-165752/verify_phase13a3bii_styled_groups/report.json`) |
| `scripts/verify_phase13a3b_styles.py` (E4 `group_effects` now equals the crop) / `verify_phase13a3_groups.py` (G5 8 bits now equals the crop) / `verify_phase13a2b_byte_styles.py` / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` | ALL GATES PASS each (same stamp, one folder per drive) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (same stamp) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | passed (56 groups + pass 2: 1; `…/20260923-165752/ui-probe/`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files, independent oracle = the full render cropped
and top-left-cell sampled, the 13A-2 windows plus windows straddling the
grown extent, divisors {1, 2, 3, 7, u32::MAX}), at 8 bits AND u16: every
13A-3b-i effect on a lone styled group of a shaped raster and a partly
off-canvas translucent one, at opacity 1, 0.6 Normal and Multiply (the
package) and Dissolve 0.6; fill 0.5 + Blend If + effects (typed Blend If on
the group at u16); the group's mask at feather 0 and 6; mixed children (a clip
unit, a hue/saturation adjustment, a feather-6 masked raster, a gated
pass-through subgroup, a Multiply isolated subgroup, a hidden and a
zero-opacity raster) in all four variants; the styled group as a clip base
(a member exceeding its extent, a clamp-only member, the clamp-only step
alone), as a clip member under an unstyled and a styled base, inside plain
and gated pass-through and Multiply isolated groups, and nested (styled
inside styled, the outer at 1 and in its package); plan rows (the `"effects"`
group row, `Global {"group_materialization"}`, children rows naming their
group, `["layer_style_planes", "group_materialization"]` with a styled raster
below, the whole-canvas input, a feathered group mask adding no halo); the
exact restatements — 8 bits `600·16 + 300·4 + style_scratch(20×15)` over a
Multiply isolated and a gated subgroup, typed `canvas·16·(1 + 2) + canvas·4·1
+ style_scratch(canvas)` and `canvas + collect work + P·18` over a Multiply
subgroup holding a clip unit and a gated subgroup; the 30000 × 1073 `"fill"`
group (Global, whole-canvas input, no style term); refusals in order (the
group's knockout before its child's, a knockout and a guided child inside by
index, a hidden styled group unchecked); the nesting-limit sentence under a
styled outermost group; linear light refused; pre-cancelled controls; the
float policy. BUDGET (8 bits, `…_with_group_budget` vs
`flattened_with_group_budget`, all three refusing with one sentence below
the peak, admitting and equal at it): a lone 20×15 group (299 / 300); nested
isolated 10×8 and gated 0.5 subgroups (599 / 600); a visible zero-opacity
Shallow child in a plain pass-through subgroup (599 / 600, D5 off); a styled
group member after a styled group base (299 / 300 — the max, not the sum); a
styled member under an EMPTY base group (299 admits everywhere and equals a
reference that renders nothing for the unit; unclipped, the same member
refuses at 299); the stated difference (899).

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 ROOT samples once the loop has begun (the materialization and
plane rendering before the loop are the reference's own preparation).
MEASURED by `cd core && cargo test --release --test regional_byte_styled_groups
a_concurrently -- --nocapture` on 3000×2000 documents of a background plus a
styled SoftLight 0.9 group (drop shadow + Multiply colour overlay — the
package path) over a 2960×1960 raster with every seventh pixel transparent
(the u16 twin the same shape; the two tests run concurrently), 3b-i's `timed`
probe past the pre-loop fraction. 8 bits: cancel-to-return 0.064 ms and 0.038
ms on two runs against 75.45 / 75.67 ns per root sample (loop 452.7 / 454.0
ms) — ≈844 and ≈508 root samples after the cancel; u16: 0.123 ms and 0.097 ms
against 79.82 / 80.04 ns (loop 478.9 / 480.2 ms) — ≈1535 and ≈1215 root
samples. All inside the declared 4096. Load-sensitive, of the same class as
the known cancellation flakes.

Drive gates (fixed before measuring, EXACT): SG1 8 and 16 bits —
`group_layers` over two `place_image` rasters (Normal at 16) +
`set_layer_style {drop_shadow, stroke, color_overlay}` on the group equals
the crop, then opacity 0.6, then Multiply, then an ellipse `add_layer_mask`
(`select_ellipse` feather 6, `set_mask_properties` feather 6); SG2 8 and 16
bits — the styled group as a clip base with a visible member; SG3 8 and 16
bits — nested in an outer group; SG4 8 and 16 bits — `{knockout: "shallow",
effects}` on the group refuses "carries a knockout" naming the group;
`list_history` unchanged by every regional call. MCP `render` returns the
image, never the plan, so `"group_materialization"` is asserted by the Rust
tests.

## 13A-3c verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the three nits (the `Styled` doc, the typed rewrap, the stated difference), before any behaviour change | 2377 passed, 0 failed (248 binaries) — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_byte_knockout --test regional_precision_knockout` (new) | 23 / 20 passed |
| `cargo test --release --test regional_byte_effects --test regional_precision_styles --test regional_groups --test regional_precision_groups --test regional_byte_styled_groups --test regional_precision_styled_groups --test regional_byte_styles --test regional_masks_clips --test regional_precision_masks_clips --test regional_document --test regional_precision --test regional_render --test knockout_composite --test precision_knockout --test knockout_merge --test integration` | 16 / 12 / 17 / 12 / 22 / 13 / 10 / 17 / 13 / 13 / 10 / 5 / 21 / 16 / 11 / 25 passed (the eight knockout / grouped-interior refusal tests are now admission-equals-reference, their ORDER points re-targeted to a guided Shadows/Highlights member) |
| `make lint` | clean |
| `make test` final | 2420 passed, 0 failed (250 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3c_knockout.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3c/20260923-181618/verify_phase13a3c_knockout/report.json`) |
| `scripts/verify_phase13a3bii_styled_groups.py` (SG4 now equals the crop) / `verify_phase13a3b_styles.py` (E4 now equals the crop) / `verify_phase13a2b_byte_styles.py` (S4 now equals the crop) / `verify_phase13a3_groups.py` / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` | ALL GATES PASS each (same stamp, one folder per drive) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (same stamp) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 55 passed, 1 failed: `current-layer-commands-ui` — "The owned editor never became the real key window (active=false, visible=true, windowKey=false, keyWindow=nil)"; re-run ALONE (`RZ_PAINT_UI_CASES=current-layer-commands-ui`) with the same failure while the machine's HID idle time was ≈48 minutes — the known screen-lock / unattended-session activation case, not a code failure (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3c/20260923-181618/ui-probe/`, `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-3c/20260923-181618/ui-probe-rerun/`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files, independent oracle = the full render cropped
and top-left-cell sampled, the 13A-2 windows plus windows straddling the
knockout layer's rect, divisors {1, 2, 3, 7, u32::MAX}), at 8 bits AND u16:
a lone Shallow (fill 0.5, fill 0 = the hole) and Deep (fill 0.5, 0, 1) raster
over D5's Background at opacity 1 and 0.7, the hole asserted to show the
Background's own pixel (8 bits) / alpha (u16); each disqualifier of
`background_entry` — offset, opacity 0.9, an enabled mask, entry 1 clipped,
hidden, Multiply, a style, a Blend If (typed Blend If at u16) — the hole
asserted transparent, Shallow too; Blend If beside a knockout (with a drop
shadow); Dissolve (with effects); masked (feather 0 and 6) and partly
off-canvas knockout layers; the package (0.6 Normal / Multiply with a drop
shadow, a Multiply colour overlay and an inner shadow); D4 — every knockout in
a plain, gated (0.5) and masked pass-through group (the Shallow hole asserted
to show the sample as the children began), in nested pass-through groups (the
inner plain and gated 0.6), a pass-through group with NO Shallow child under a
level that has one, inside isolated (Multiply, Normal 0.8) and materialized
groups (the Deep hole asserted to show what lies beneath the group); knockout
MEMBERS under an unstyled base, a styled base (fill, Blend If, effects, each
at 1 and 0.6), a masked (feather 6) base, after a first member that altered
the buffer, as a group member (effect-less and materialized) and under an
adjustment base; a knockout clip base with a visible and a clamp-only member,
the clamp-only member alone, and in its package; D6 (colour overlay Multiply,
gradient overlay, satin, inner glow grouped; inner shadow, inside stroke and
bevel outside; fill 0.5) on a lone Multiply / Normal raster and in its
package, beside a Deep knockout, on a base WITH a visible member (off), with
hidden-only members (on), on a member (on), on a materialized group and beside
a Shallow knockout there; the plan's row shape unchanged and no scratch term
for a knockout outside a materialized subtree; the typed materialized-subtree
restatement (+16·canvas for a Deep child, +20·canvas for a grouped child, +0
for a grouped clip base with a visible member, +32·canvas for a Shallow
member, rows unchanged); linear light refused; pre-cancelled controls; the
float fixture refusing a knockout and a grouped interior style. BUDGET (8
bits): a visible opacity-1 Shallow child in a materialized nest's pass-through
subgroup refuses at 599 (reference, plan, render) and equals at 600; the
STATED DIFFERENCE pinned on a 100×100 canvas — a Deep raster, and the twin
with a plain pass-through group holding a visible Shallow child, under an
isolated drop-shadow group over a 100×60 child: `flattened_with_group_budget(
12000)` refuses with the budget sentence while every window of
`render_region_with_group_budget(.., 12000, ..)` equals the reference at
`MAX_GROUP_BUFFER_PIXELS`. A mutation check (the unit pass reading `target` in
place of `backdrop_at`) failed 14 tests in each new file before it was
reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 ROOT samples once the loop has begun. MEASURED by `cd core &&
cargo test --release --test regional_byte_knockout a_concurrently --
--nocapture` on 3000×2000 documents of a Background plus a Shallow (fill 0.5)
raster at opacity 0.9 over 2960×1960 pixels with every seventh pixel
transparent (the u16 twin the same shape; the two tests run concurrently),
3b-i's `timed` probe. 8 bits: cancel-to-return 0.005 ms and 0.028 ms on two
runs against 49.49 / 50.49 ns per root sample (loop 296.9 / 302.9 ms) — ≈107
and ≈559 root samples after the cancel; u16: 0.109 ms and 0.107 ms against
59.98 / 61.35 ns (loop 359.9 / 368.1 ms) — ≈1821 and ≈1750 root samples. All
inside the declared 4096. Load-sensitive, of the same class as the known
cancellation flakes.

Drive gates (fixed before measuring, EXACT, 8 and 16 bits): K1 — a
canvas-sized opaque `place_image` at (0, 0) + a placed raster styled
`{knockout: shallow, fill_opacity: 0.5, effects: []}` equals the crop, then
`{knockout: deep, fill_opacity: 0}`, then `{knockout: shallow, effects:
[drop_shadow, color_overlay]}` at opacity 0.6; K2 — a knockout member under a
clip base (`set_layer_clipped`), then the base styled `{fill_opacity: 0.5,
blend_if}`; K3 — a Shallow child inside a pass-through `group_layers` group,
then the group Multiply; K4 — `{blend_interior_as_group: true, effects:
[color_overlay multiply]}` on a lone Multiply raster, then as a clip base with
a visible member; `list_history` unchanged by every regional call. SG4 / E4 /
S4 of the earlier drives now assert equality with the crop where they asserted
the refusal.

## 13A-4a verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the three 13A-3c nits (`base_only_backdrop`, `s * opacity`, the helpers moved into `regional_knockout.rs`), before any behaviour change | 2420 passed, 0 failed, 2 ignored — the byte-identity check |
| `make test` after the guide hoists (`tone_plane_with_job`, `Adjustment::guide_radius` / `guide_with_job`, `adjust_precision_plan::guide` visibility), before any behaviour change | 2420 passed, 0 failed, 2 ignored — the byte-identity check |
| `cd core && cargo fmt --check` | clean |
| `cargo test --release --test regional_byte_guide --test regional_precision_guide` (new) | 20 / 19 passed |
| `cargo test --release --test regional_document --test regional_precision --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test regional_byte_effects --test regional_precision_styles --test regional_groups --test regional_precision_groups --test regional_byte_styled_groups --test regional_precision_styled_groups --test regional_byte_knockout --test regional_precision_knockout --test regional_render --test integration --test adjustment_tests --test style_cache` | 13 / 10 / 17 / 13 / 10 / 16 / 12 / 17 / 12 / 22 / 13 / 23 / 20 / 5 / 25 / 10 / 11 passed (the two lone-root guided refusals are now admission-equals-reference; every order test unchanged) |
| `make lint` | clean |
| `make test` final | 2459 passed, 0 failed, 2 ignored |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4_guide.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4a/20260923-193643/verify_phase13a4_guide/report.json`; F1 refused at the EDIT with "the shadows_highlights adjustment is unavailable in a 32-bit float document"). A first attempt (`…/13a-4a/20260923-193623/`) stopped at F1 on a drive-script bug (a Python keyword clash in the `add_adjustment_layer` call), fixed before the recorded run; G1–G5 had passed there too |
| `verify_phase13a3c_knockout.py` / `verify_phase13a3bii_styled_groups.py` / `verify_phase13a3b_styles.py` / `verify_phase13a3_groups.py` / `verify_phase13a2b_byte_styles.py` / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` | ALL GATES PASS each (same stamp, one folder per drive) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (same stamp) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4a/20260923-193643/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files; independent oracle = the full render cropped
and top-left-cell sampled, the 13A-2 windows plus windows whose plane is a
STRICT sub-window of a canvas three halos wide, divisors {1, 2, 3, 7,
u32::MAX}), at 8 bits AND u16: radii 4 (factor 1), 8 (2), 16 (4), 30 (8,
grid direct), 120 (8, grid sigma > 3 — the byte grid blur reduces again) and
1000 (the plane is the canvas), each on a canvas ≥ 3× its halo (radius 1000:
203×157) and not a multiple of 8, over a backdrop with transparent holes (a
lattice and a large block — the coverage-0 fallback) and translucent pixels
under an Overlay raster, the guide asserted to change the projection; the
interior window's plane asserted strict (and the canvas at radius 1000);
masked (feather 0 and 6), opacity 0.7, Multiply, Dissolve; two guides with a
SoftLight raster between (`stages[1].plane ⊆ stages[0].plane ⊆
required_input_region`, both planes restated); a guide above a knockout
Background (D5 in stage 0, `S` charged as `|R_0|·16`); a baseless clipped
guide at the root; a guided base over a non-empty run (8 bits: visible and
hidden member, the unit contributes nothing, full render asserted equal to
the stack without it; u16: a visible member contributes nothing, a
hidden-only run applies and is staged); an EMPTY isolated (Normal) group —
no children, and hidden-only children — below a radius-8 guide, the plan's
`stages[0].layer` asserted to be the guide's stack / record index (3, 4);
refusals by index inside a group and as a clip member; zero-amount
Shadows/Highlights pointwise with no stage; the exact plan restatements —
`stages`, `border_policy`, the row and top-level `neighborhood`,
`required_input_region`, scratch and work — over three windows / divisors,
the guide figures from the shared typed planner and the geometry written out
independently in the tests; the halo composition — a feather-P raster below a
radius-4 guide `P + H` (and `required_input_region` grown by it), a guided
row with its own feather-P mask `max(P, H)`, two guides with a feather-P
raster between `max(P + H_2, H_1 + H_2)`, a pure guide pair `H_1 + H_2`;
`with_evaluation(LinearLight)` `Err`; pre-cancelled controls; float — the
Shadows/Highlights layer lands on the fixture but the full render, the plan
and the render all refuse with the float policy's sentence. MUTATION CHECK:
with `plane_region`'s lattice set to 1 (no alignment),
`a_radius_8_guide_at_factor_2_matches_the_projection` failed at window (95,
0, 6, 5) divisor 1, and its u16 twin at the same window, before it was
reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples of the running stage loop, or after the running byte
`blur_plane` (uncancellable, 13A-5) completes. MEASURED by `cd core && cargo
test --release --test regional_byte_guide a_concurrently -- --nocapture` on
3000×2000 documents (a Background, a Multiply raster at 0.9 over 2960×1960
with every seventh pixel transparent, a radius-30 guide at 0.8, a SoftLight
raster above; the whole canvas at divisor 1, so `R_0` is the canvas), 3c's
`timed` probe, cancelling once during stage 0 and once during the final
stage; samples-after-cancel are `latency / (render time / staged samples)`,
the whole render's AVERAGE per staged sample (the two tests run
concurrently). Four runs:

| Run | 8-bit stage 0 | 8-bit final stage | u16 stage 0 | u16 final stage |
|---|---|---|---|---|
| 1 | 0.068 ms, ≈1112 | 0.028 ms, ≈463 | 0.079 ms, ≈1576 | 0.211 ms, **≈4206** |
| 2 | 0.014 ms, ≈244 | 0.008 ms, ≈141 | 0.059 ms, ≈1205 | 0.132 ms, ≈2679 |
| 3 | 0.010 ms, ≈168 | 0.047 ms, ≈812 | 0.080 ms, ≈1620 | 0.019 ms, ≈376 |
| 4 | 0.042 ms, ≈720 | 0.085 ms, ≈1458 | 0.053 ms, ≈1075 | 0.178 ms, ≈3602 |

(8 bits ≈58–61 ns and u16 ≈49–50 ns per staged sample; render 696–732 ms /
590–603 ms.) Fifteen of sixteen inside the declared 4096; ONE u16
final-stage figure, ≈4206, is a MISS by this estimator — recorded, not
widened. The estimator divides by the whole render's average per-sample cost,
which includes the cheaper stage-0 samples and the fill passes, so it
over-counts samples in the dearer final stage; the u16 checkpoint is one
`progress.advance` per 4096-sample chunk. Load-sensitive, of the same class as
the known cancellation flakes. After the 13A-4a review the u16 final-stage
estimate is calibrated on the final stage's OWN uncancelled loop (the same
probe point to the return, over the canvas); re-run: 65.28 ns per final-stage
sample (loop 391.7 ms) against the stage-0 estimate's 50.94 ns, cancel-to-return
0.186 ms, ≈2853 samples — inside the declared 4096 (the other three columns
unchanged in method; that run: 8-bit ≈102 / ≈3247, u16 stage 0 ≈159).

Drive gates (fixed before measuring, EXACT, 8 and 16 bits): G1 — a 256×192
transparent `new_document` + a canvas-sized placed backdrop with holes and
translucent pixels + an Overlay raster + three `add_adjustment_layer
shadows_highlights` layers (radius 4 / 30 / 120), toggled visible one at a
time then all, each equals the crop; G2 — the radius-30 layer with an
ellipse `add_layer_mask` from a selection, `set_mask_properties` feather 6,
opacity 0.7, Multiply; G3 — a guided layer inside `group_layers` and as a clip
member refuse "is Shadows/Highlights with a guide" naming the layer, and as a
clip base with a visible member equals the crop; G4 — zero-amount
Shadows/Highlights; G5 — an empty `group_layers` group (its only child
deleted, set to Normal) below the radius-30 layer; F1 — the float fixture
refuses the op by the float policy's sentence. `list_history` unchanged by
every regional call.

## 13A-4b-i verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the two 13A-4a nits (the per-sample bodies moved to `doc_regional_sample.rs` / `precision_regional_style.rs`; the 8-bit final-stage calibration), before any behaviour change | 2459 passed, 0 failed — the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_nested_guide --test regional_precision_nested_guide` (new) | 15 / 13 passed |
| `cargo test --release --test regional_byte_guide --test regional_precision_guide --test regional_document --test regional_precision --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test regional_byte_effects --test regional_precision_styles --test regional_groups --test regional_precision_groups --test regional_byte_styled_groups --test regional_precision_styled_groups --test regional_byte_knockout --test regional_precision_knockout --test regional_render` | 20 / 19 / 13 / 10 / 17 / 13 / 10 / 16 / 12 / 17 / 12 / 22 / 13 / 23 / 20 / 5 passed. Flipped to admission-equals-reference: `regional_precision_guide` (a guide in an isolated group), `regional_byte_styled_groups` / `regional_precision_styled_groups` (a guided child of a materialized group) and — not named by the spec — `regional_groups`' re-targeted order case (a guided child of a group isolated by its visible knockout member); the two 13A-4a plan-restatement tests' exact `"stages"` JSON gained `"level": null` and `"buffer"`. Every pass-through and clip-member refusal unchanged |
| `make test` final | 2487 passed, 0 failed |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4b_nested_guide.py` | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4b-i/20260923-211024/verify_phase13a4b_nested_guide/report.json`; F1 refused at the EDIT) |
| `verify_phase13a4_guide.py` (G3 `in_group` still refused: Pass Through) / `verify_phase13a3c_knockout.py` / `verify_phase13a3bii_styled_groups.py` / `verify_phase13a3b_styles.py` / `verify_phase13a3_groups.py` / `verify_phase13a2b_byte_styles.py` / `verify_phase13a2_masks_clips.py` / `verify_phase13a1_regional.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each (same stamp, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-4b-i/20260923-211024/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files; independent oracle = the full render cropped
and top-left-cell sampled; windows = 13A-4a's plus windows straddling each
corner of the group's rect and windows wholly outside it; divisors {1, 2, 3,
7, u32::MAX}), at 8 bits AND u16: an isolated (Normal) group whose rect is
STRICTLY inside the canvas and NOT lattice-aligned (offset (37, 29)) holding a
holed raster and a guide at radii 4, 30 and 120 (the guide asserted to change
the projection); a group touching the canvas edge; nested isolated groups each
with a guide (the inner one below the outer's guide), without a root guide
(`stages[0].level` the outer group, `stages` pre-order, both planes, halo,
scratch and work restated) and with one (levels null, outer, inner); an
unguided isolated group holding a guided one; a group guide between root
guides below and above it (its target the root's `R_1`); two guides in one
group with a raster between; the group as a clip base with a visible member,
as a clip member, inside plain and gated (opacity 0.6, mask) pass-through
groups, masked (feather 6) at opacity 0.7 Multiply, with a fill / Blend If
style; a window missing the 8-bit rect (no stage for that level; at u16 the
canvas-buffered level is still staged, asserted); a MATERIALIZED group
holding a guided child, holding an isolated group holding a guide and holding
a pass-through group holding a guide (admitted, EXACT, `stages` empty); a
guided base over a non-empty run inside a group (no stage, pointwise row);
refusals by index — a guided clip member inside an isolated and inside a
materialized group, a guided child directly in a plain and in a gated
pass-through group; the plan restatements written out independently
(`level`, `buffer`, `plane` from a buffer-anchored `plane_region`,
`cell_pixels`, `reach_cells`, `required_input_region` ⊇ every plane,
`halo_pixels`, scratch, work) over five windows / divisors per radius,
including partial-overlap and wholly-outside windows. MUTATION CHECKS: (a)
anchoring nested planes at the canvas origin (clamped to the buffer) failed
ten of the thirteen non-cancel byte nested tests — the radius-4 one at its
plan restatement, the others inspected at the pixel comparison — while every
13A-4a root test still passed; (b)
popping the typed level AFTER the recursion failed
`nested_isolated_groups_each_with_a_guide_match_with_and_without_a_root_guide_at_u16`
with "regional stage mismatch" (window (3, 2, 11, 7), divisor 1). Both
reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples of the running stage loop (nested included) or after the
running byte `blur_plane` / `rendered_group` completes. MEASURED by `cd core
&& cargo test --release --test regional_byte_nested_guide a_concurrently --
--nocapture` on 3000×2000 documents (a Background; an isolated group holding
a Multiply raster at 0.9 over 2957×1963 with holes and a radius-30 guide; a
SoftLight raster above; the whole canvas at divisor 1, no root guide), each
stage's per-sample estimate calibrated on its OWN uncancelled loop (the time
between the progress point that opens it and the one that closes it, over its
samples), cancelling once in the nested stage 0, once in the nested held
plane and once in the root loop. Two runs:

| Run | 8-bit nested stage 0 | 8-bit held plane | 8-bit root loop | u16 nested stage 0 | u16 held plane | u16 root loop |
|---|---|---|---|---|---|---|
| 1 | 0.049 ms, ≈2935 | 0.053 ms, ≈1174 | 0.020 ms, ≈386 | 0.014 ms, ≈829 | 0.046 ms, ≈1120 | 0.079 ms, ≈2035 |
| 2 | 0.009 ms, ≈494 | 0.064 ms, ≈1451 | 0.035 ms, ≈679 | 0.013 ms, ≈779 | 0.021 ms, ≈525 | 0.068 ms, ≈1724 |

(8 bits ≈17 / 44–45 / 51–52 ns and u16 ≈16–17 / 41 / 39 ns per staged
sample.) All twelve inside the declared 4096; no miss. The 13A-4a nit: `cd
core && cargo test --release --test regional_byte_guide a_concurrently --
--nocapture` with the 8-bit final stage calibrated on its own loop — 76.25 ns
per final-stage sample (loop 457.5 ms), 0.024 ms cancel-to-return, ≈310
samples (8-bit stage 0 ≈422; u16 stage 0 ≈601, u16 final ≈3167) — inside the
declared 4096.

Drive gates (fixed before measuring, EXACT, 8 and 16 bits, `list_history`
unchanged by every regional call): N1 — `group_layers` + `set_layer_properties
blend_mode Normal` around a holey raster placed at (37, 29) and a radius-30
guide; N2 — an isolated group (a raster + a radius-8 guide) inside an isolated
group (a raster, the inner group, a radius-30 guide), then with a radius-4
root guide on top; N3 — the N1 group as a clip base with a visible member,
then with an ellipse `add_layer_mask`, `set_mask_properties` feather 6,
opacity 0.7, Multiply; N4 — a guide inside a Pass Through `group_layers` group
and as a clip member still refuse by name; N5 — a `set_layer_style`
drop-shadow group holding the guide, and one holding a Normal group that
holds it; F1 — the float fixture refuses the op by the float policy's
sentence.

## 13A-4b-ii-a verification record (2026-09-23)

| Check | Result |
|---|---|
| `make test` after the walk split (`doc_regional_walk.rs` / `precision_regional_walk.rs`, moved verbatim), before any behaviour change | 2487 passed, 0 failed — the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_pass_through_guide --test regional_precision_pass_through_guide` (new) | 19 / 18 passed |
| `cargo test --release --test regional_byte_guide --test regional_precision_guide --test regional_byte_nested_guide --test regional_precision_nested_guide --test regional_groups --test regional_precision_groups --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test regional_precision_styles --test regional_byte_effects --test regional_byte_knockout --test regional_precision_knockout --test regional_byte_styled_groups --test regional_precision_styled_groups --test regional_document --test regional_precision --test regional_render` | 20 / 19 / 15 / 13 / 17 / 12 / 17 / 13 / 10 / 12 / 16 / 23 / 20 / 22 / 13 / 13 / 10 / 5 passed. Flipped to admission-equals-reference (the named flips): `regional_byte_guide`'s `group_layers` half, `regional_byte_nested_guide` / `regional_precision_nested_guide`'s pass-through halves, `regional_groups` / `regional_precision_groups`' guided pass-through child (their now-unused `assert_refuses` helpers removed for `make lint`); the exact stage JSON of `regional_byte_guide` / `regional_precision_guide` and the nested files' `stage()` helpers gained `"through": []`. Every clip-member refusal unchanged |
| `make test` final | 2524 passed, 0 failed |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4bii_pass_through.py` (new) | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4b-ii-a/20260923-223631/verify_phase13a4bii_pass_through/report.json`; F1 refused at the EDIT) |
| `verify_phase13a4b_nested_guide.py` (N4 `in_pass_through` flipped to equal the crop) / `verify_phase13a4_guide.py` (G3 `in_group` flipped to equal the crop) / `verify_phase13a3c_knockout.py` / `verify_phase13a3_groups.py` / `verify_phase13a1_regional.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each (same stamp, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-4b-ii-a/20260923-223631/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files; independent oracle = the full render cropped
and top-left-cell sampled; windows = 13A-4b-i's (13A-4a's plus windows
straddling each corner of the content rect and windows wholly outside it);
divisors {1, 2, 3, 7, u32::MAX}), at 8 bits AND u16: a plain and an opacity-0.6
root Pass Through group holding a holed raster at the odd offset (37, 29) and
a guide at radii 4, 30 and 120 (the guide asserted to change the projection;
the plan restated over four windows / divisors each: `level` null, `buffer`
the canvas, the canvas-anchored `plane`, `through == [3]`, halo,
`required_input_region` ⊇ the plane, scratch with `16·|R_0|` and work with
`|R_0|` iff held — at 8 bits only the gated group, at u16 both); gated by a
feather-6 mask and by mask and opacity (8 bits: exactly one hold over the
same doc's mask-only difference; a disabled mask holds nothing); a Shallow
knockout raster beside the guide (8 bits: a PLAIN group held for its Shallow
child, +`16·|R_0|` / +`|R_0|` exactly); nested pass-throughs — plain in
plain, plain in gated, gated in plain, both gated (`through == [5, 3]`; 8
bits one hold unless both are gated; u16 always two); a gated pass-through
inside an isolated group (level = the group, the 8-bit buffer its rect; a
window missing the rect stages nothing at 8 bits, the canvas-buffered typed
level is still staged) and the same under a root guide (the root's level,
then the group's over root `R_0` ∩ rect); an isolated group whose ONLY guide
sits inside its pass-through child (plain and gated; staged as the group's
level — objection 2); an isolated guided group inside a plain and a gated
pass-through beside a second guide (two staged levels, pre-order: root
`through == [5]`, then the group's `through == []`); two guides in one plain,
gated and gated-and-masked pass-through with a raster between (one hold,
entered before P_0 and resumed by both loops); a gated pass-through guide
between root guides below and above (its hold entered by loop 0 over `R_1`,
charged over `R_0`); D5's Background under a guided pass-through (a Shallow
child inside it, plain and gated; a Deep knockout at the root below it,
plain and gated; the Background snapshot one more `16·|R_0|`); a pass-through
clip base with a hidden member (absorbed) and with a VISIBLE member
(isolated at both depths, `level` = the group — objection 3 pinned); hidden
and zero-opacity pass-throughs (8 bits skipped, no stage; u16 hidden skipped,
zero opacity staged and gated all the way back — the typed skip is `visible`
alone — both EXACT); a guided clip member inside a plain and a gated
pass-through still refusing by index; a pass-through guide inside a
MATERIALIZED group (admitted, no stage, the guide's halo declared); linear
light refusing the document at both depths; the float fixture refusing the
guided op by the float policy's sentence at the full render, the plan and the
render. MUTATION CHECKS: (a) dropping the byte FINISH's store failed 11 of the
19 byte tests (every test holding a gated case); (b) dropping the Shallow reason from
`pass_through_holds` failed
`a_shallow_knockout_child_beside_the_guide_holds_the_plain_group_sample`.
Both reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples of the running stage loop (nested and pass-through paths
and hold fills included), or after the running byte `blur_plane` /
`rendered_group` completes. MEASURED by `cd core && cargo test --release
--test regional_byte_pass_through_guide a_concurrently -- --nocapture` on
3000×2000 documents (a Background; a radius-30 root guide; a GATED (0.6) Pass
Through group holding a Multiply raster at 0.9 over 2957×1963 with holes and a
radius-30 guide; a SoftLight raster above; the whole canvas at divisor 1 —
cursors `[1]` then `[2, 1]`, so the group's hold is filled before loop 0 and
read by the final loop's FINISH), each stage's per-sample estimate calibrated
on its OWN uncancelled loop (the time between the progress point that opens
it and the one that closes it, over its samples; the 8-bit hold fill reports
no progress, so its window is guide 1's report to loop 0's first row — the
fill plus one row), cancelling once in stage 0, once in the hold fill, once
in the held gated stage (loop 0, entering the group) and once in the root
loop (the final stage). Run 1 and run 3 are the command as written (the 8-bit
and u16 tests concurrently); run 2 added `--test-threads=1`:

| Run | 8-bit stage 0 | 8-bit hold fill | 8-bit held gated stage | 8-bit root loop | u16 stage 0 | u16 hold fill | u16 held gated stage | u16 root loop |
|---|---|---|---|---|---|---|---|---|
| 1 | 0.011 ms, ≈507 | 0.009 ms, ≈23060 | 0.214 ms, ≈2074 | 0.212 ms, ≈1798 | 0.061 ms, ≈2966 | 0.010 ms, ≈30240 | 0.341 ms, ≈3571 | 0.385 ms, ≈3229 |
| 2 | 0.050 ms, ≈2420 | 0.009 ms, ≈24660 | 0.258 ms, ≈2400 | 0.315 ms, ≈2899 | 0.020 ms, ≈1029 | 0.013 ms, ≈42588 | 0.254 ms, ≈2697 | 0.437 ms, ≈3736 |
| 3 | 0.043 ms, ≈1949 | 0.011 ms, ≈29114 | 0.275 ms, ≈2555 | 0.260 ms, ≈2174 | 0.082 ms, ≈4109 | 0.009 ms, ≈21421 | 0.372 ms, ≈3608 | 0.463 ms, ≈3704 |

(8 bits ≈21–22 / 0.35–0.37 / 103–108 / 109–120 ns and u16 ≈19–21 / 0.30–0.44
/ 94–103 / 117–125 ns per sample.) MISSES, recorded and not widened: every
HOLD-FILL cancel converts to 21,421–42,588 "samples" — the cancel-to-return
itself is 0.009–0.013 ms, at the fixed return floor the stage loops also show
(0.011 ms at best), while a fill sample costs a third of a nanosecond; the
fill polls every 4096 samples by construction (`plane` / `zero_plane`). And
run 3's u16 stage 0 converts to ≈4109 (13 samples over, with the 8-bit
measurement running concurrently). The other 22 are inside the declared 4096.

Drive gates (fixed before measuring, EXACT, 8 and 16 bits, `list_history`
unchanged by every regional call): P1 — a radius-30 guide and a holey raster
at (37, 29) grouped by a default `group_layers` (Pass Through); P2 — that
group at opacity 0.6 with an ellipse `add_layer_mask`, `set_mask_properties`
feather 6; P3 — the P1 group inside a Normal group, and a Normal group (a
raster + a radius-8 guide) inside a Pass Through group beside a radius-30
guide; P4 — a guide clipped to a raster inside a Pass Through group still
refuses by name; F1 — the float fixture refuses the op by the float policy's
sentence.

## 13A-4b-ii-b-1 verification record (2026-09-24)

| Check | Result |
|---|---|
| `make test` after the moves (`regional_plan.rs`, the byte cursor block to `doc_regional_sample.rs`, the typed cursor block to `precision_regional_sample.rs`), before any behaviour change | 2524 passed, 0 failed — the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_clip_guide --test regional_precision_clip_guide` (new) | 20 / 20 passed |
| `cargo test --release --test regional_byte_guide --test regional_precision_guide --test regional_byte_nested_guide --test regional_precision_nested_guide --test regional_byte_pass_through_guide --test regional_precision_pass_through_guide --test regional_masks_clips --test regional_precision_masks_clips --test regional_byte_styles --test regional_precision_styles --test regional_byte_effects --test regional_groups --test regional_precision_groups --test regional_byte_knockout --test regional_precision_knockout --test regional_document --test regional_precision --test regional_render` | 20 / 19 / 15 / 13 / 19 / 18 / 17 / 13 / 10 / 12 / 16 / 17 / 12 / 23 / 20 / 13 / 10 / 5 passed. Flipped (the named flips): a guide clipped to a plain raster (`regional_byte_guide` / `regional_precision_guide`), a guided member beside a knockout MEMBER (`regional_byte_styles` / `regional_precision_masks_clips`), a guided member in an isolated and a materialized group (`regional_byte_nested_guide` / `regional_precision_nested_guide`; materialized: `stages == []`) and in a plain and a gated pass-through (`regional_byte_pass_through_guide` / `regional_precision_pass_through_guide`) now equal the reference, those tests renamed and their now-unused `assert_refuses` helpers removed for `make lint`; a guided member under a STYLED (knockout) base (`regional_masks_clips`, `regional_byte_effects`, `regional_precision_styles`) now refuses "is Shadows/Highlights with a guide under a styled base" |
| `make test` final | 2564 passed, 0 failed |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4biib_clip_members.py` (new) | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4b-ii-b-1/20260924-000859/verify_phase13a4biib_clip_members/report.json`; F1 refused at the EDIT) |
| `verify_phase13a4_guide.py` (G3 `as_member` flipped to equal the crop) / `verify_phase13a4b_nested_guide.py` (N4 `as_member` flipped) / `verify_phase13a4bii_pass_through.py` (P4 flipped) / `verify_phase12e2b_placement.py` | ALL GATES PASS each (same stamp, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-4b-ii-b-1/20260924-000859/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files; independent oracle = the full render cropped
and top-left-cell sampled; windows = 13A-4b-i's (13A-4a's plus windows
straddling each corner of the content rect and windows wholly outside it);
divisors {1, 2, 3, 7, u32::MAX}), at 8 bits AND u16, EXACT: a guide clipped
to a holey Overlay raster at (37, 29) at the root at radii 4, 30 and 120 (the
guide asserted to change the projection; the plan restated over four windows
/ divisors each in the `assert_pass_plan` form: `level` null, the canvas
buffer, the canvas-anchored plane, `through == []`, the composed halo,
`required_input_region` ⊇ the plane, scratch with ONE hold `16·|R_0|`, work
with `rows_0 = 2` (backdrop, base) over `R_0`, the member stage's `rows = 2`
(base and guide, from the unit's first row) over the output, the guide, the
fill and the hold `|R_0|`); the spec's example (base row 0, member row 1:
`rows_0 = 1`, `stages[0].rows = 2`); raster members below and above the guide
(`rows_0 = 3`, stage rows 4); two guides in one unit (one hold; stage rows 3
and 4, both from the unit's first row); a Shallow knockout member below and
above the guide (8 bits `wants_base`, typed `members_want_base_snapshot16`);
a typed Blend If base (`min(a)` clamp, the weight against the held began
sample on the re-run); the member masked (feather 6) at 0.7 Multiply; a
hidden raster member (dropped) and a zero-opacity one (8 bits clamp-only,
typed kept) before the guide; the guided member itself hidden (no stage) and
at opacity 0 (8 bits clamp-only, no stage; typed staged and skipped); an
isolated GROUP base (its two children charged: `rows_0 = 4`, stage rows 4;
also at opacity 0.6); the unit inside an isolated level (the group's level,
the 8-bit buffer its rect; a window missing the rect stages nothing at 8
bits) and the same under a root guide above (two levels); the unit inside a
plain, a gated and a gated-and-masked pass-through (`through == [3]`; holds 8
bits 1 plain / 2 gated, u16 always 2) and nested plain-in-gated (`through ==
[4, 3]`; holds 2 / 3); the unit between root guides below and above (three
stages, rows 2 / 3 / 1, the hold entered by loop 0 and charged over `R_0`);
D5's Background under a unit with a Shallow member and under a Deep root
knockout below it (the snapshot one more `16·|R_0|` at 8 bits); the unit
inside a MATERIALIZED group (admitted, `stages == []`, the guide's halo
declared); a guide clipped to a Levels adjustment base (contributes nothing,
still staged, EXACT); styled bases — fill-only, effects, knockout, a
materialized group base — refusing by the new sentence at plan and render;
linear light refusing the document; the float fixture refusing the guided op
by the float policy's sentence at the full render, the plan and the render.
MUTATION CHECKS: (a) dropping the FINISH's `acc = under` store failed 15 of
the 20 byte tests and 16 of the 20 u16 tests; (b) dropping the member paths
from `guided_paths` failed the byte isolated-level test (the byte root reaches
members by `path_of_layer`) and 16 of the 20 u16 tests; (c) counting the
member stage from the member's own index failed 11 of the 20 byte tests (the
restated work figures). All reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples of the running stage loop (clip-unit re-runs and hold
fills included), or after the running byte `blur_plane` / `rendered_group`
completes. MEASURED by `cd core && cargo test --release --test
regional_byte_clip_guide a_concurrently -- --nocapture` on 3000×2000
documents (a Background; a radius-30 root guide; a Multiply raster at 0.9
over 2957×1963 with holes and a radius-30 guide CLIPPED to it; a SoftLight
raster above; the whole canvas at divisor 1 — cursors `[1]` then `[2, 0]`, so
the unit's began sample is held before loop 0, which enters it, and the final
loop re-runs it), each stage calibrated on its OWN uncancelled loop, cancelling
once in stage 0, once in the unit's hold fill, once in the entering stage
(loop 0) and once in the root loop (the resumed unit). Run 1 and run 3 are the
command as written (both depths concurrently); run 2 added
`--test-threads=1`:

| Run | 8-bit stage 0 | 8-bit hold fill | 8-bit entering stage | 8-bit root loop | u16 stage 0 | u16 hold fill | u16 entering stage | u16 root loop |
|---|---|---|---|---|---|---|---|---|
| 1 | 0.029 ms, ≈1247 | 0.013 ms, ≈32702 | 0.275 ms, ≈2685 | 0.072 ms, ≈621 | 0.045 ms, ≈2350 | 0.012 ms, ≈35044 | 0.250 ms, ≈2804 | 0.014 ms, ≈120 |
| 2 | 0.010 ms, ≈439 | 0.017 ms, ≈46471 | 0.191 ms, ≈1870 | 0.116 ms, ≈1015 | 0.063 ms, ≈3303 | 0.019 ms, ≈62024 | 0.226 ms, ≈2040 | 0.049 ms, ≈425 |
| 3 | 0.062 ms, ≈2979 | 0.034 ms, ≈91068 | 0.253 ms, ≈2447 | 0.019 ms, ≈170 | 0.061 ms, ≈3398 | 0.011 ms, ≈27014 | 0.274 ms, ≈2967 | 0.021 ms, ≈179 |

(8 bits ≈21–24 / 0.38–0.41 / 102–103 / 114–117 ns and u16 ≈18–19 /
0.30–0.39 / 89–111 / 115–120 ns per sample.) MISSES, recorded and not
widened: every HOLD-FILL cancel converts to 27,014–91,068 "samples" — the
cancel-to-return is 0.011–0.034 ms, the fixed return floor, while a fill
sample costs about a third of a nanosecond (the known 13A-5 metric item). The
other 18 are inside the declared 4096.

Drive gates (fixed before measuring, EXACT, 8 and 16 bits, `list_history`
unchanged by every regional call): C1 — a radius-30 guide `set_layer_clipped`
to a holey raster at (37, 29); C2 — that unit inside a default `group_layers`
(Pass Through) and inside a Normal group; C3 — a raster member clipped above
the guide and the guide at opacity 0.6 Multiply; C4 — the base given
`set_layer_style` `{"effects":[{"type":"drop_shadow"}]}` refuses "… under a
styled base" naming the guide; F1 — the float fixture refuses the op by the
float policy's sentence.

## 13A-4b-ii-b-2 verification record (2026-09-24)

| Check | Result |
|---|---|
| `make test` after the moves (`plan_levels` / `plan_level` / `input_halo` to `regional_plan.rs`, the budget dry run to `doc_regional_budget.rs`), before any behaviour change | 2564 passed, 0 failed — equal to the pre-move (13A-4b-ii-b-1 final) count: the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_styled_clip_guide --test regional_precision_styled_clip_guide` (new) | 15 / 13 passed (the byte file includes both concurrent-cancel measurements) |
| `cargo test --release --test regional_byte_clip_guide --test regional_precision_clip_guide --test regional_byte_effects --test regional_masks_clips --test regional_precision_styles --test regional_byte_styles --test regional_byte_knockout --test regional_precision_knockout` | 20 / 20 / 16 / 17 / 12 / 10 / 23 / 20 passed. Flipped (the named flips): `styled_bases_equal_the_reference_and_a_packaged_base_refuses` (byte: fill-only, Shallow knockout and a materialized group base equal the reference; effects at 0.85 refuse `PACKAGED_BASE`), `styled_bases_equal_the_reference_at_u16` (all four equal), and the guided member under a knockout base in `regional_byte_effects` / `regional_masks_clips` / `regional_precision_styles` now equals the reference; the now-unused `assert_refuses` helpers of the last three and of `regional_precision_clip_guide` removed for `make lint` |
| `make test` final | 2592 passed, 0 failed |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4biib2_styled_members.py` (new) | ALL GATES PASS (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-4b-ii-b-2/20260924-013223/verify_phase13a4biib2_styled_members/report.json`; F1 refused at the EDIT) |
| `verify_phase13a4biib_clip_members.py` (C4 flipped to equal the crop) / `verify_phase13a4_guide.py` / `verify_phase13a4b_nested_guide.py` / `verify_phase13a4bii_pass_through.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each (same stamp, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-4b-ii-b-2/20260924-013223/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `region` description); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (both new files; independent oracle = the full render cropped
and top-left-cell sampled; windows = 13A-4b-ii-b-1's; divisors {1, 2, 3, 7,
u32::MAX}), EXACT at 8 bits and u16, the plan restated in the
`assert_root_plan` form (stages, halo, input region — the whole canvas for a
style with planes — the work whole, the staged scratch as the difference from
the same document with the guides hidden, which carries the style's own
scratch and, typed, its preparation work): a guide clipped to a fill-only
base (8 bits also with a Blend If); an effects base (drop shadow + stroke) at
opacity 1 (the guide asserted to change the projection); effects at 0.85 —
8 bits refusing `PACKAGED_BASE` at plan and render naming the member (the
base alone admitted), u16 equal (the typed package) — and effects at 0.85 in
Dissolve equal at both depths; a Shallow knockout base without planes (routed)
and with effects at opacity 1 (u16 also at 0.85, packaged); Deep, Shallow and
Shallow-with-effects knockout bases under D5's Background (the snapshot one
more `16·|R_0|`); a materialized group base (u16 also at 0.85); raster members
below and above the guide (`rows_0 = 3`, stage rows 4); two guides in one unit
(one hold; stage rows 3 and 4); a Shallow knockout member below and above the
guide; the member masked (feather 6) at 0.7 Multiply; a 14×12 Shallow
knockout patch with a member spilling far past it and whole-canvas windows
(the guide reads the unit buffer outside the rect); the unit inside an
isolated level (the group's level staged) and inside a plain and a gated
pass-through (`through == [3]`; holds 8 bits 1 / 2, u16 2); linear light
refusing the document; the float fixture refusing the guided op by the float
policy's sentence at the full render, the plan and the render.
MUTATION CHECKS: (a) dropping the `stop` early return before
`composite_source_into` (byte `direct_sample` and typed `direct_sample16`)
failed 8 of the 13 byte tests and 8 of the 13 u16 tests; (b) dropping `acc[i]
= under` in the typed styled dispatch failed 11 of the 13 u16 tests; (c)
making the unit pass's early returns unconditional (today's order under
`Some(stop)` too) failed 3 of the 13 byte and 3 of the 13 u16 tests — the
small-patch case, the knockout bases and the D5 knockout bases. All reverted.

**Measured cancellation bound.** DECLARED before measuring: a cancel returns
within 4096 samples of the running stage loop (clip-unit re-runs and hold
fills included), or after the running byte `blur_plane` / `rendered_group` /
`LayerStyle::rendered` completes. MEASURED by `cd core && cargo test
--release --test regional_byte_styled_clip_guide a_concurrently --
--nocapture` on 3000×2000 documents (a Background; a radius-30 root guide; a
Multiply raster over 2957×1963 with holes, styled, and a radius-30 guide
CLIPPED to it; a SoftLight raster above; the whole canvas at divisor 1 —
cursors `[1]` then `[2, 0]`), two variants per depth: an EFFECTS base (drop
shadow + stroke, opacity 1: the styled-direct member loop) and a DEEP
knockout base (fill 0.6, opacity 0.9: routed, D5 firing). Each stage is
calibrated on its OWN uncancelled loop (the u16 probes shifted by the style's
preparation work the typed plan counts). Runs 1 and 3 are the command as
written; run 2 added `--test-threads=1`. Cancel-to-return, ≈samples after the
cancel:

| Run | Variant | 8-bit stage 0 | 8-bit hold fill | 8-bit entering stage | 8-bit root loop | u16 stage 0 | u16 hold fill | u16 entering stage | u16 root loop |
|---|---|---|---|---|---|---|---|---|---|
| 1 | effects | 0.012 ms, ≈549 | 0.011 ms, ≈28282 | 0.211 ms, ≈1650 | 0.023 ms, ≈165 | 0.022 ms, ≈1186 | 0.012 ms, ≈38755 | 0.392 ms, ≈3275 | 0.067 ms, ≈471 |
| 1 | Deep | 0.087 ms, ≈2951 | 0.009 ms, ≈23186 | 0.276 ms, ≈2666 | 0.065 ms, ≈549 | 0.034 ms, ≈1166 | 0.009 ms, ≈24458 | 0.214 ms, ≈2186 | 0.117 ms, ≈946 |
| 2 | effects | 0.044 ms, ≈2062 | 0.009 ms, ≈22803 | 0.290 ms, ≈2261 | 0.072 ms, ≈500 | 0.042 ms, ≈2368 | 0.010 ms, ≈29587 | 0.340 ms, ≈2827 | 0.175 ms, ≈1316 |
| 2 | Deep | 0.024 ms, ≈812 | 0.011 ms, ≈29578 | 0.259 ms, ≈2532 | 0.044 ms, ≈380 | 0.035 ms, ≈1241 | 0.009 ms, ≈32494 | 0.361 ms, ≈3837 | 0.107 ms, ≈888 |
| 3 | effects | 0.036 ms, ≈1646 | 0.008 ms, ≈19919 | 0.169 ms, ≈1169 | 0.016 ms, ≈116 | 0.065 ms, ≈3564 | 0.011 ms, ≈27211 | 0.287 ms, ≈2273 | 0.061 ms, ≈446 |
| 3 | Deep | 0.037 ms, ≈1204 | 0.009 ms, ≈23209 | 0.175 ms, ≈1714 | 0.064 ms, ≈528 | 0.034 ms, ≈1227 | 0.009 ms, ≈27242 | 0.270 ms, ≈2992 | 0.097 ms, ≈800 |

(Per sample: 8 bits ≈21–23 / 0.38–0.39 / 128–144 / 136–145 ns with effects
and ≈29–31 / 0.37–0.38 / 102–103 / 117–122 ns Deep; u16 ≈18 / 0.31–0.39 /
120–126 / 133–143 ns with effects and ≈28–29 / 0.28–0.37 / 90–98 / 121–124 ns
Deep.) MISSES, recorded and not widened: every HOLD-FILL cancel converts to
19,919–38,755 "samples" — the cancel-to-return is 0.008–0.012 ms, the fixed
return floor, while a fill sample costs about a third of a nanosecond (the
known 13A-5 metric item). The other 36 are inside the declared 4096.

Drive gates (fixed before measuring, EXACT, 8 and 16 bits, `list_history`
unchanged by every regional call): S1 — a radius-30 guide clipped to a holey
raster given `{"fill_opacity":0.5}`; S2 — the base given
`{"effects":[{"type":"drop_shadow"}]}` at opacity 1; S3 — the base given
`{"fill_opacity":0.5,"knockout":"shallow"}`; S4 — the S2 base at
`set_layer_properties` opacity 0.6: 8 bits refuses "… under a packaged styled
base (8-bit effects below opacity 1)" naming the guide, 16 bits equals the
crop; S5 — the guide clipped to a Normal group holding the raster, the group
given the S2 effects; F1 — the float fixture refuses the op by the float
policy's sentence.

## 13A-5a verification record (2026-09-24)

| Check | Result |
|---|---|
| `make test` after step 2 (`checkpoints_only`, `bytes_with_job`, `blur_plane_with_job`, `derived_with_job`, the u8 guide blur threading and the `render_staged` narrowing), before any regional-tree change | 2592 passed, 0 failed — equal to the 13A-4b-ii-b-2 final count: the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_preparation_cancel -- --nocapture` (new) | 6 passed (twice: runs 1 and 2 below) |
| the same with `--test-threads=1` | 6 passed (run 3 below) |
| `cargo test --release --test regional_byte_pass_through_guide --test regional_byte_clip_guide --test regional_byte_styled_clip_guide a_concurrently -- --nocapture` | 2 / 2 / 2 passed; every probe fired in its own stage (every loop's `c` agrees with its u16 twin's); all 32 stages PASS the model (table below) |
| `cargo test --release --test regional_masks_clips --test regional_byte_guide --test regional_byte_nested_guide` | 17 / 20 / 15 passed |
| `make test` final | 2598 passed, 0 failed (2592 + the new file's 6) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a2_masks_clips.py` / `verify_phase13a4_guide.py` / `verify_phase13a4bii_pass_through.py` / `verify_phase13a4biib_clip_members.py` / `verify_phase13a4biib2_styled_members.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-5a/20260924-023920/`, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-5a/20260924-023920/ui-probe/report.json`) |
| `scripts/generate_mcp_reference.py` (port 4917, dummy key) then `--check` | regenerated (the `render` description's "(not Dissolve)"); "MCP reference is current"; the instance terminated, `pgrep` empty |

Rust coverage (`core/tests/regional_byte_preparation_cancel.rs`; 3000×2000
fixtures, whole canvas, divisor 1; (a) a Background and a SoftLight raster
with an enabled canvas-sized mask at feather 6 — sigma 2.3, factor 1 — and
feather 40 — sigma 12.5, factor 8; (b) the 13A-4a guided root fixture,
radius 30): the regional render through the POLLED path (a live, uncancelled
control, a cold cache per render) equals `rz_doc_flattened`'s crop at both
feathers over the whole canvas and two sub-windows; a pre-cancelled control
returns "operation cancelled", no image, no scratch, at once, on all three
documents; CACHE INTEGRITY — a cancel half-way into the blur of document A,
then an uncancelled regional render of A equals the full render of an
independently built B, at both feathers (every timed run builds its fixture
anew: the derived-mask cache is an `Arc` every clone shares); REPORTS-ONCE
GUARDS (deterministic, no cancel, a spinning poller) — on (b) the first
progress value above stage 0's close `a0/work` equals the guide's report
`(a0 + g0)/work` to 1e-6, and on (a) the first value above 0 is the same
fraction feathered and unfeathered, at both feathers.

**Measured cancellation (the 13A-5a model).** DECLARED before measuring:
`T_f` = 1 ms; `W` = 4096 cells per `feather` pass for the factor-1 mask
blur, ONE Triangle resample for the factor-8 mask blur (the larger of the
down and up passes, timed by the test on the extended plane's dimensions),
4096 cells for the byte guide build. `t_blur` = the feathered uncancelled
run's first-progress-report time minus the unfeathered one's; the factor-1
`c` = `t_blur / (2·pw·ph)` (pw × ph = 3014 × 2014); the guide's `c` = the
window from stage 0's last report to the guide's report over `|R_0|` (6 MP).
These stages report nothing, so each cancel is issued at a TIME probe
half-way into its calibrated window, and the progress read just before the
cancel is ASSERTED to be the target stage's (exactly 0 for the mask blurs,
before the first row reports; stage 0's close, strictly below the guide's
report, for the guide build). MEASURED by the two commands above;
runs 1 and 2 are the command as written, run 3 `--test-threads=1`:

| Run | Stage | Cancel-to-return | `c` | Bound `T_f + W·c` | Past the floor | Verdict |
|---|---|---|---|---|---|---|
| 1 | mask blur, factor 1 (t_blur 188.0 ms) | 0.036 ms | 15.488 ns / cell | 1.063 ms | 0 cells | PASS |
| 1 | mask blur, factor 8 (t_blur 106.0 ms) | 19.454 ms | 60.803 ms / resample (down 11.793) | 61.803 ms | 0.30 resamples | PASS |
| 1 | byte guide build (window 16.1 ms) | 0.010 ms | 2.686 ns / sample | 1.011 ms | 0 | PASS |
| 2 | mask blur, factor 1 (t_blur 94.6 ms) | 0.033 ms | 7.795 ns / cell | 1.032 ms | 0 cells | PASS |
| 2 | mask blur, factor 8 (t_blur 62.4 ms) | 33.954 ms | 56.899 ms / resample (down 11.754) | 57.899 ms | 0.58 resamples | PASS |
| 2 | byte guide build (window 14.9 ms) | 0.016 ms | 2.479 ns / sample | 1.010 ms | 0 | PASS |
| 3 | mask blur, factor 1 (t_blur 109.6 ms) | 0.027 ms | 9.024 ns / cell | 1.037 ms | 0 cells | PASS |
| 3 | mask blur, factor 8 (t_blur 59.0 ms) | 29.603 ms | 56.930 ms / resample (down 12.386) | 57.930 ms | 0.50 resamples | PASS |
| 3 | byte guide build (window 15.4 ms) | 0.013 ms | 2.571 ns / sample | 1.011 ms | 0 | PASS |

The factor-8 cancels land in the up resample (the unpolled unit) and return
at its end, inside one resample. Cache-integrity cancels (issued at, blur
window, returned after): run 1 feather 6 at 131.4 ms (≈11.9–250.9 ms) 0.029
ms, feather 40 at 47.7 ms (≈2.3–93.2) 24.038 ms; run 2 67.9 ms (≈19.3–116.5)
0.033 ms, 39.1 ms (≈2.1–75.9) 21.555 ms; run 3 62.2 ms (≈8.3–116.0) 0.042
ms, 32.7 ms (≈2.2–63.1) 27.984 ms — every cancel inside the blur, and every
re-render equal to the reference.

**The re-based 4b-ii tables under the model** (`a_concurrently`, the
command above; cancel-to-return / bound, every one PASS, 0 samples past the
floor):

| File | Stage | 8-bit | u16 |
|---|---|---|---|
| pass-through (13A-4b-ii-a) | stage 0 | 0.061 / 1.088 ms | 0.017 / 1.074 ms |
| | hold fill (c 0.36 / 0.41 ns) | 0.009 / 1.001 ms | 0.017 / 1.002 ms |
| | held gated stage (loop 0) | 0.199 / 1.433 ms | 0.212 / 1.369 ms |
| | root loop | 0.221 / 1.488 ms | 0.363 / 1.481 ms |
| clip member (13A-4b-ii-b-1) | stage 0 | 0.066 / 1.086 ms | 0.008 / 1.073 ms |
| | the unit's hold fill (c 0.39 / 0.34 ns) | 0.009 / 1.002 ms | 0.010 / 1.001 ms |
| | entering stage (loop 0) | 0.223 / 1.409 ms | 0.247 / 1.384 ms |
| | root loop | 0.038 / 1.461 ms | 0.023 / 1.467 ms |
| styled member, effects (13A-4b-ii-b-2) | stage 0 | 0.013 / 1.090 ms | 0.040 / 1.076 ms |
| | the unit's hold fill (c 0.39 / 0.41 ns) | 0.009 / 1.002 ms | 0.015 / 1.002 ms |
| | entering stage (loop 0) | 0.335 / 1.528 ms | 0.389 / 1.496 ms |
| | root loop | 0.032 / 1.549 ms | 0.170 / 1.555 ms |
| styled member, Deep (13A-4b-ii-b-2) | stage 0 | 0.009 / 1.125 ms | 0.043 / 1.113 ms |
| | the unit's hold fill (c 0.38 / 0.36 ns) | 0.010 / 1.002 ms | 0.011 / 1.001 ms |
| | entering stage (loop 0) | 0.159 / 1.420 ms | 0.239 / 1.367 ms |
| | root loop | 0.069 / 1.473 ms | 0.080 / 1.501 ms |

The byte hold fill now reports, and its calibrated `c` (0.36–0.39 ns) sits
beside the u16 twin's (0.34–0.41 ns), as do every byte loop's `c` and its
u16 twin's — the reports-once evidence that each probe fires in its own
stage. The hold-fill MISSES of 13A-4b-ii-a … 13A-4b-ii-b-2 are PASSes under
the model: the same ≈0.01 ms returns sit under the 1 ms floor. Known load
flakes: the two reports-once guards in `regional_byte_preparation_cancel.rs`
(`the_byte_guide_blur_reports_nothing_after_stage_0`,
`the_mask_blur_reports_nothing_before_the_first_row`) are LOAD-SENSITIVE as
written: they assert the first value a spinning poller observes, and
`fetch_max` overwrites the guide's single report with the next row's ~0.1 ms
later. In the fix pass they failed 5 of 9 parallel runs at load average ≈59
(first value 0.545478 = the next loop row, not a leak) and passed every
`--test-threads=1` run and 3/3 at load 25–27. Not a product defect; the
load-independent guard is an open item owned by 13A-5b.

## 13A-5b-i verification record (2026-09-24)

| Check | Result |
|---|---|
| `make test` after steps 2–5 (`style_plane_ops.rs`, the `_with_job` renderers, the cache, the call sites), before the new test file | 2598 passed, 0 failed — equal to the 13A-5a final count: the byte-identity check |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `regional_byte_preparation_cancel` built once (`--no-run`), the BINARY run 4× concurrently for 3 rounds (`uptime` before each round: load 9.5 / 13.8 / 27.3), then the guards alone (`leaks`) 4× concurrently for 3 rounds (load 36.3 / 42.5 / 42.5) | the two new guards passed 24/24 in the full-binary rounds and 12/12 in the guard-only rounds; the full binary passed 7/12 — the other 5 failed 13A-5a's `a_concurrently_cancelled_preparation_blur_returns_within_the_model` on its own stage assertion ("the cancel landed outside the target stage", a TIME probe calibrated on separate runs landing past the blur under load), not a guard |
| `cargo test --release --test regional_byte_preparation_cancel -- --nocapture --test-threads=1` | 6 passed (load ≈41) |
| `cargo test --release --test regional_style_preparation_cancel -- --nocapture` (new) | 7 passed (twice: runs 1 and 2 below) |
| the same with `--test-threads=1` | 7 passed (run 3 below) |
| `cargo test --release --test style_cache --test style --test style_bevel_emboss --test style_inner_glow --test style_inner_shadow --test style_outer_glow --test style_satin --test style_stroke --test style_color_overlay --test style_gradient_overlay --test precision_layer_styles --test regional_byte_styles --test regional_precision_styles --test regional_byte_styled_groups --test regional_precision_styled_groups --test regional_byte_styled_clip_guide --test regional_precision_styled_clip_guide` | 11 / 26 / 13 / 14 / 13 / 13 / 11 / 17 / 14 / 20 / 9 / 10 / 12 / 22 / 13 / 15 / 13 passed |
| `make test` final | 2605 passed, 0 failed (2598 + the new file's 7) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3b_styles.py` / `verify_phase13a3bii_styled_groups.py` / `verify_phase13a4biib2_styled_members.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each (`~/devel/rasterize-fixtures/phase13-receipts/13a/13a-5b-i/20260924-040043/`, one folder per drive, logs in `logs/`) |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 (same stamp) |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (`…/13a-5b-i/20260924-040043/ui-probe/report.json`) |
| `wc -l core/src/style_render.rs` | 498 (670 before; `style_plane_ops.rs` 261) |

Rust coverage (`core/tests/regional_style_preparation_cancel.rs`; 3000×2000,
whole canvas, divisor 1; a Background and a canvas-sized Normal raster with
anti-aliased holes carrying (c) a drop shadow, distance 0, size 4 — sigma 2,
factor 1, plane 3016×2016 — and size 40 — sigma 20, factor 8, plane
3164×2164 — with a size-0 twin; (d) the same at u16; (e) an inner bevel of
size 4, 8 bits, plane 3016×2016): the regional render through the POLLED
path (a live, uncancelled control; a fresh document per byte render, a cold
plane cache) equals `rz_doc_flattened`'s crop (8 bits) and
`rz_precision_doc_render`'s samples (u16) over the whole canvas and two
sub-windows ((e): the whole canvas and one sub-window); a pre-cancelled
control returns "operation cancelled", no image, no scratch, within 100 ms on
all five documents; CACHE INTEGRITY — a cancel half-way into A's plane render,
then an uncancelled regional render of A, equals the full render of an
independently built B, at both sizes; the INTERVAL + PIN guards — byte pre 0
/ target `1/H` on (c) at both sizes and (e); u16 pre `pixels/work` (the zero
fill's close) / target `1 − 2·pixels/work` (the style's `advance`; `work ≥
3·pixels` asserted) on (d) at both sizes.

**Measured cancellation (the 13A-5a model, the 13A-5b-i intervals).**
DECLARED before measuring (the contract deltas above): `T_f` = 1 ms; `T_0` /
`T_S` the first report above the pre-stage value in the size-0 / size-S
uncancelled runs (fresh fixtures), `t_style = T_S − T_0`, the cancel issued
at `T_0 + t_style/2` with progress asserted 0 (byte) / `pre ± 1e-6` (u16).
MEASURED by the commands above; runs 1 and 2 are the command as written, run
3 `--test-threads=1`:

| Run | Row | `t_style` | Cancel-to-return | Calibrated costs | Bound | Intervals past the floor | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | 8-bit drop shadow 4 (factor 1) | 115.9 ms | 0.017 ms | c 9.534 ns / cell, c_map 1.801 ms | 2.801 ms | 0.00 | PASS |
| 1 | u16 drop shadow 4 (factor 1) | 87.5 ms | 0.034 ms | c 7.195 ns / cell, c_map 1.801 ms | 2.801 ms | 0.00 | PASS |
| 1 | 8-bit drop shadow 40 (factor 8) | 61.8 ms | 30.205 ms | c_resample 58.821 ms (down 12.001), c_map 1.669 ms | 59.821 ms | 0.50 | PASS |
| 1 | u16 drop shadow 40 (factor 8) | 61.6 ms | 30.784 ms | c_resample 82.633 ms (down 12.012), c_map 1.669 ms | 83.633 ms | 0.36 | PASS |
| 1 | 8-bit inner bevel 4 (factor 1) | 118.8 ms | 0.029 ms | t_style / ph 0.059 ms, c_map 1.497 ms | 2.497 ms | 0.00 | PASS |
| 2 | 8-bit drop shadow 4 (factor 1) | 100.7 ms | 0.029 ms | c 8.281 ns / cell, c_map 1.589 ms | 2.589 ms | 0.00 | PASS |
| 2 | u16 drop shadow 4 (factor 1) | 172.3 ms | 0.023 ms | c 14.171 ns / cell, c_map 1.589 ms | 2.589 ms | 0.00 | PASS |
| 2 | 8-bit drop shadow 40 (factor 8) | 68.1 ms | 27.516 ms | c_resample 58.720 ms (down 12.130), c_map 1.711 ms | 59.720 ms | 0.45 | PASS |
| 2 | u16 drop shadow 40 (factor 8) | 62.3 ms | 31.925 ms | c_resample 58.818 ms (down 11.932), c_map 1.711 ms | 59.818 ms | 0.53 | PASS |
| 2 | 8-bit inner bevel 4 (factor 1) | 119.6 ms | 0.014 ms | t_style / ph 0.059 ms, c_map 1.517 ms | 2.517 ms | 0.00 | PASS |
| 3 | 8-bit drop shadow 4 (factor 1) | 93.6 ms | 0.015 ms | c 7.701 ns / cell, c_map 1.457 ms | 2.457 ms | 0.00 | PASS |
| 3 | u16 drop shadow 4 (factor 1) | 93.6 ms | 0.025 ms | c 7.694 ns / cell, c_map 1.457 ms | 2.457 ms | 0.00 | PASS |
| 3 | 8-bit drop shadow 40 (factor 8) | 64.9 ms | 27.953 ms | c_resample 59.003 ms (down 12.084), c_map 1.865 ms | 60.003 ms | 0.46 | PASS |
| 3 | u16 drop shadow 40 (factor 8) | 61.9 ms | 31.002 ms | c_resample 58.951 ms (down 12.426), c_map 1.865 ms | 59.951 ms | 0.51 | PASS |
| 3 | 8-bit inner bevel 4 (factor 1) | 118.3 ms | 0.042 ms | t_style / ph 0.059 ms, c_map 1.528 ms | 2.528 ms | 0.00 | PASS |

The factor-8 cancels land in the up resample (the unpolled unit) and return
at its end, inside one resample. The machine was shared during these runs
(load average 28–42 from other work). Cache-integrity cancels (issued at,
plane window, returned after): run 1 size 4 at 100.4 ms (≈27.2–173.5 ms)
0.016 ms, size 40 at 37.9 ms (≈7.1–68.7) 66.465 ms; run 2 69.0 ms
(≈24.5–112.9) 0.053 ms, 44.3 ms (≈15.3–73.3) 26.900 ms; run 3 58.1 ms
(≈14.3–101.9) 0.037 ms, 38.8 ms (≈7.0–70.5) 29.235 ms — every cancel inside
the plane render at progress 0, and every re-render equal to the reference.
Known load flake (report, not a defect of this run): 13A-5a's
`a_concurrently_cancelled_preparation_blur_returns_within_the_model` fails
its own stage assertion under 4× parallel load (above); it passed the serial
run.

## 13A-5b-ii-a verification record (2026-09-24)

Receipts: `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-5b-ii-a/20260924-052731/`
(`cargo/` the Rust logs and `cargo/par/` the 24 parallel-round logs, `logs/`
the drives, one folder per drive, `ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` after steps 1–5 (the counter, the env field, the mask twins, the `Result` ripple and polls, `resolve_materialized`), before any test file | 2605 passed, 0 failed — equal to the 13A-5b-i final count: the byte-identity check |
| `cargo test --release --test regional_byte_preparation_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 6 passed each |
| `cargo test --release --test regional_group_materialization_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 7 passed each |
| both built once (`--no-run`); each BINARY run 4× concurrently for 3 rounds (`uptime` before each round: preparation 5.2 / 5.6 / 9.9, materialization 14.3 / 16.7 / 36.5) | `regional_byte_preparation_cancel` 12/12 (13A-5a's `a_concurrently_cancelled_preparation_blur_returns_within_the_model` passed 12/12 — no stage-assertion or stall failure); `regional_group_materialization_cancel` 12/12 |
| `cargo test --release --test regional_byte_styled_groups --test regional_groups --test regional_masks_clips --test regional_byte_styles --test regional_byte_knockout --test regional_byte_nested_guide --test regional_byte_clip_guide --test regional_byte_styled_clip_guide --test regional_style_preparation_cancel --test group_tests --test knockout_composite --test knockout_merge --test style --test style_cache --test mask_tests --test independent_masks --test mask_outside_coverage` | 22 / 17 / 17 / 10 / 23 / 15 / 20 / 15 / 7 / 41 / 21 / 11 / 26 / 11 / 43 / 11 / 8 passed |
| `make test` final | 2612 passed, 0 failed (2605 + the new file's 7; 263 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a3bii_styled_groups.py` / `verify_phase13a3b_styles.py` / `verify_phase13a3c_knockout.py` / `verify_phase13a4biib2_styled_members.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |
| `wc -l core/src/doc_group.rs core/src/doc_mask.rs core/src/regional.rs` | 1092 (1071 before: the polls and the two mask-crop `if` rewrites only) / 520 / 497 |

Rust coverage (`core/tests/regional_group_materialization_cancel.rs`;
3000×2000, whole canvas, divisor 1): (g) a Background and an isolated group
carrying a drop shadow (distance 0, size 4) over N = 6 canvas-sized Normal
0.9 rasters with anti-aliased holes, and its N = 1 twin; (c) the N = 1 group
whose child carries an ENABLED canvas-sized mask at feather 6 (sigma 2.3,
factor 1, padded plane 3014×2014), with its feather-0 twin; (s) the N = 3 group whose top child also carries a size-4 drop shadow
(plane 3016×2016), with its size-0 twin; (h) at 300×200 a materialized group
(drop shadow, under a feathered (6) group mask) over a shaped raster under
an enabled feather-4 mask, a Multiply raster, a Levels adjustment under an
enabled feather-3 mask, a guided Shadows/Highlights (radius 30), a clip unit
(base, adjustment member, raster member under an enabled feather-3 mask), a
gated (0.6) pass-through group, a nested styled isolated group and a
Shallow-knockout styled raster under an enabled feather-3 mask — also as a
CLIP MEMBER of a raster base. EXACTNESS: the polled path (a live,
uncancelled control; fresh fixtures) equals `rz_doc_flattened`'s crop over
the whole canvas and two sub-windows for (g), (c), (s), (h) and (h) as a
member; PRE-CANCELLED: "operation cancelled", no image, no scratch, within
100 ms on all five; INTERVAL + PIN on (g) (pre 0, target `1/H`); CACHE
INTEGRITY: (s) cancelled at the inner raster's first blur report inside
`rendered_group`, then an uncancelled render equals an independently built
B; (c) cancelled half a pass into the CHILD's mask blur — the resource
report's `mask_cache_planes` is 0 — then an uncancelled render equals B and
the cache holds the plane.

**Measured cancellation (the 13A-5a model, the intervals above).** DECLARED
before measuring (`regional.rs`, the module doc of the new file). Triggers:
(m1) (g) entry `checkpoints ≥ ⌈W·H/4096⌉`, `k = N·H/2`, verdict progress 0 /
stage 1; (m2) (g) the same entry, a 5 ms freeze; (m3) (s) the flip into
stage 6 (the inner raster's blur), `k = 0`, verdict progress 0 / stage 6;
(m4) (c) the same flip (the child's mask blur), `k = ⌈pw·ph/4096⌉/2`.
MEASURED by the commands above:

| Run | Probe | Cancel-to-return | Bound | Intervals past the floor | Verdict |
|---|---|---|---|---|---|
| 1 | (m1) child row loop | 0.031 ms | 1.004 ms (`c_row`) | 0.00 | PASS |
| 1 | (m2) unpolled unit (freeze) | 0.726 ms | 8.183 ms (`c_unit`) | 0.00 | PASS |
| 1 | (m3) child style plane render | 0.034 ms | 2.505 ms | 0.00 | PASS |
| 1 | (m4) child mask blur | 0.038 ms | 1.045 ms | 0.00 | PASS |
| 2 | (m1) | 0.033 ms | 1.013 ms | 0.00 | PASS |
| 2 | (m2) | 0.958 ms | 26.545 ms | 0.00 | PASS |
| 2 | (m3) | 0.033 ms | 2.556 ms | 0.00 | PASS |
| 2 | (m4) | 0.039 ms | 1.044 ms | 0.00 | PASS |
| 3 | (m1) | 0.032 ms | 1.017 ms | 0.00 | PASS |
| 3 | (m2) | 0.830 ms | 34.928 ms | 0.00 | PASS |
| 3 | (m3) | 0.039 ms | 2.439 ms | 0.00 | PASS |
| 3 | (m4) | 0.035 ms | 1.046 ms | 0.00 | PASS |
| 4× parallel, 12 runs | (m1) | 0.015–0.038 ms | 1.002–1.065 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (m2) | 0.017–3.697 ms | 7.574–130.276 ms | ≤ 0.10 | PASS 12/12 |
| 4× parallel, 12 runs | (m3) | 0.022–0.120 ms | 2.505–8.767 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (m4) | 0.037–0.041 ms | 1.042–1.110 ms | 0.00 | PASS 12/12 |

(m2)'s bound varies with `t_mat` (T(6) − T(1) from separate uncancelled
runs: 30–170 ms serially; `c_encode` 5.5 ms); the freeze lands after the
row loops (≈13,500 checkpoints at return: the fill's 1465 plus six
children's 2000 rows each and their unit polls), in `encode_projection`,
and returns at its end. The re-measured 13A-5a
probes on the checkpoint trigger:

| Run | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| 1 / 2 / 3 | mask blur, factor 1 (feather 6) | 0.035 / 0.018 / 0.035 ms | 1.039 / 1.042 / 1.044 ms | 0 | PASS |
| 1 / 2 / 3 | mask blur, factor 8 (feather 40), freeze | 44.630 / 42.234 / 43.334 ms | 57.869 / 58.168 / 56.524 ms | 0.77 / 0.72 / 0.76 resamples | PASS |
| 1 / 2 / 3 | byte guide build (radius 30) | 0.012 / 0.013 / 0.013 ms | 1.010 ms | 0 | PASS |
| 4× parallel, 12 runs | mask blur, factor 1 | 0.034–0.087 ms | 1.034–1.096 ms | 0 | PASS 12/12 |
| 4× parallel, 12 runs | mask blur, factor 8 | 41.875–84.651 ms | 59.659–104.062 ms | ≤ 1.29 | PASS 10/12, **MISS 2/12** (82.7 ms vs 64.4 ms; 84.7 ms vs 72.3 ms) |
| 4× parallel, 12 runs | byte guide build | 0.009–0.025 ms | 1.010–1.018 ms | 0 | PASS 12/12 |

The two factor-8 MISSES are recorded, not widened: the freeze probe lands in
the up resample and the cancel returns at its end, but under 4× parallel
load the worker's resample ran slower than the test thread's own timing of
it (`resample`, calibrated once per test) — the verdict line prints MISS
and the test does not fail on it (it asserts the stage, which held 12/12).
Cache-integrity cancels (13A-5a file, the flip, `k` 0): every cancel inside
the blur at progress 0 / stage 6, every re-render equal to the reference.

## 13A-5b-ii-b verification record (2026-09-24)

Receipts: `~/devel/rasterize-fixtures/phase13-receipts/13a/13a-5b-ii-b/20260924-061853/`
(`cancel/` the three suites' runs 1, 2 and serial, `parallel/` the 48
parallel-round logs and `uptime.log`, `regression-suites.log`,
`make-test-final.log`, `typecheck.log`, `make-app.log`, `logs/` the drives,
one folder per drive, `ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` after steps 1–2 (the primitives and every caller, the probe move), before the new test file | 2612 passed, 0 failed — equal to the 13A-5b-ii-a final count: the byte-identity check |
| `cargo test --release --test regional_byte_primitive_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 5 passed each |
| `cargo test --release --test regional_group_materialization_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 7 passed each |
| `cargo test --release --test regional_style_preparation_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 7 passed each |
| the four cancel binaries built once (`--no-run`); each BINARY run 4× concurrently for 3 rounds (`uptime` before each round: preparation 30.3 / 38.6 / 36.8, primitive 35.2 / 35.2 / 37.9, materialization 37.9 / 42.0 / 48.0, style 49.2 / 52.6 / 55.6 — the machine was shared) | `regional_byte_preparation_cancel` 12/12, `regional_byte_primitive_cancel` 12/12, `regional_group_materialization_cancel` 12/12, `regional_style_preparation_cancel` 12/12 (no stage-assertion or stall failure) |
| `cargo test --release --test regional_byte_styled_groups --test regional_groups --test regional_masks_clips --test regional_byte_styles --test regional_byte_knockout --test regional_byte_nested_guide --test regional_byte_clip_guide --test regional_byte_styled_clip_guide --test regional_precision_styles --test regional_precision_styled_groups --test regional_precision_knockout --test group_tests --test knockout_composite --test knockout_merge --test style --test style_cache --test mask_tests --test independent_masks --test mask_outside_coverage` | 356 passed, 0 failed (19 binaries) |
| `make test` final | 2617 passed, 0 failed (2612 + the new file's 5; 264 binaries) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` / `verify_phase13a3b_styles.py` / `verify_phase13a3bii_styled_groups.py` / `verify_phase13a3c_knockout.py` / `verify_phase13a4biib2_styled_members.py` | ALL GATES PASS each, exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |
| `wc -l core/src/style_composite.rs core/src/doc_group.rs core/src/doc_mask.rs core/src/regional.rs` | 999 (964 before) / 1101 (1092) / 527 (520) / 502 (497) |

Rust coverage (`core/tests/regional_byte_primitive_cancel.rs`; 3000×2000,
whole canvas, divisor 1): (q) a Background; A, a styled clip base (colour
overlay, a non-identity Blend If, fill 0.8, opacity 0.5 — the package path)
over a canvas-sized raster with anti-aliased holes and a canvas-sized raster
member; B, a fill-0.9 clip base (no effects → `Shape::of_layer(.., 0)`) over
an 1800×1300 shaped raster with a 1500×1100 member; A and B in an isolated
group carrying a drop shadow (size 4) and an enabled canvas-sized mask at
feather 0, density 0.9. EXACTNESS: the polled path (a live, uncancelled
control; fresh fixtures) equals `rz_doc_flattened`'s crop over the whole
canvas and two sub-windows; PRE-CANCELLED: "operation cancelled", no image,
no scratch, within 100 ms; INTERVAL + PIN (pre 0, target `1/H`); the
ZERO-WIDTH gated pass: a 0×3 canvas, a pushed 1×1 child (`adding_image_layer`
refuses the canvas) grouped at opacity 0.6, `flattened()` returns a 0×3
image (the same test panics at `doc_group.rs`'s `chunks_exact_mut` with the
guard removed — checked once, then restored).

**Measured cancellation (the 13A-5a model, the 13A-5b-ii-b intervals).**
DECLARED before measuring (the contract deltas above). Triggers: (q)
calibration `window(past_the_fill(W, H), stage == 6)` → `t_chain`, `n_chain`
(31792 checkpoints every run); four probes past the fill plus `⌊n_chain·j/8⌋`
checkpoints, j ∈ {1, 3, 5, 7}, verdict progress 0 / stage 1, bound `T_f +
c_row`, `c_row = t_chain / n_chain`. MEASURED by the commands above; runs 1
and 2 are the command as written, run 3 `--test-threads=1`:

| Run | Probe | `c_row` (`t_chain`) | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|---|
| 1 | (q) 1/8 / 3/8 / 5/8 / 7/8 | 7.832 µs (249.0 ms) | 0.017 / 0.032 / 0.012 / 0.022 ms | 1.008 ms | 0.00 | PASS ×4 |
| 2 | (q) 1/8 / 3/8 / 5/8 / 7/8 | 8.032 µs (255.4 ms) | 0.017 / 0.028 / 0.012 / 0.013 ms | 1.008 ms | 0.00 | PASS ×4 |
| 3 | (q) 1/8 / 3/8 / 5/8 / 7/8 | 7.594 µs (241.4 ms) | 0.017 / 0.027 / 0.012 / 0.013 ms | 1.008 ms | 0.00 | PASS ×4 |
| 4× parallel, 12 runs | (q) 1/8 | — | 0.017–0.047 ms | 1.016–1.022 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (q) 3/8 | — | 0.015–0.045 ms | 1.016–1.022 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (q) 5/8 | — | 0.010–0.029 ms | 1.016–1.022 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (q) 7/8 | — | 0.011–0.022 ms | 1.016–1.022 ms | 0.00 | PASS 12/12 |

The materialization file's probes ((m2) re-pointed at (g′), bound `T_f +
c_copy`, freeze `min(5 ms, c_copy/2)`; (m1), (m3), (m4) unchanged):

| Run | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| 1 / 2 / 3 | (m1) child row loop | 0.028 / 0.030 / 0.029 ms | 1.013 / 1.015 / 1.017 ms | 0.00 | PASS |
| 1 / 2 / 3 | (m2) (g′) freeze (c_copy 1.833 / 1.862 / 1.944 ms) | 1.013 / 1.007 / **5.818** ms | 2.833 / 2.862 / 2.944 ms | 0.01 / 0.00 / 2.48 | PASS / PASS / **MISS** |
| 1 / 2 / 3 | (m3) child style plane render | 0.032 / 0.031 / 0.031 ms | 2.507 / 2.451 / 2.477 ms | 0.00 | PASS |
| 1 / 2 / 3 | (m4) child mask blur | 0.039 / 0.036 / 0.036 ms | 1.043 ms | 0.00 | PASS |
| 4× parallel, 12 runs | (m1) | 0.017–0.075 ms | 1.000–1.063 ms | 0.00 | PASS 12/12 (one round's calibration saturated: T(1) > T(6) under load, `t_mat` 0, bound 1.000 ms, "NaN" intervals printed) |
| 4× parallel, 12 runs | (m2) | 0.060–5.246 ms | 2.887–9.864 ms | ≤ 1.90 | PASS 11/12, **MISS 1/12** (5.246 ms vs 3.236 ms) |
| 4× parallel, 12 runs | (m3) | 0.030–0.035 ms | 2.505–8.346 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | (m4) | 0.034–0.042 ms | 1.028–1.082 ms | 0.00 | PASS 12/12 |

(m2)'s freeze fires 8 checkpoints past the fill (1465 → 1473 at return in
every serial run), before any row loop — consistent with the gated pass's
entry poll followed by its unpolled `acc.to_vec()` of the group's freshly
allocated buffer (not proven by the count alone). `c_copy` is timed on the
test thread over an already-written source; the two MISSES (5.8 ms, 5.2 ms)
are recorded, not widened (open item below).

The style file's probes on the checkpoint trigger (bounds and cost
calibration unchanged, time-based):

| Run | Probe | Trigger | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|---|
| 1 / 2 / 3 | 8-bit drop shadow 4 (factor 1) | `k` 6047 after `at_the_blur` (3480) | 0.031 / 0.035 / 0.036 ms | 2.549 / 2.677 / 2.501 ms | 0.00 | PASS |
| 1 / 2 / 3 | u16 drop shadow 4 (factor 1) | `k` 6054 after `pre` (1469) | 0.031 / 0.033 / 0.032 ms | 2.549 / 2.677 / 2.501 ms | 0.00 | PASS |
| 1 / 2 / 3 | 8-bit drop shadow 40 (factor 8) | 5 ms freeze after `at_the_blur` (7810) | 42.617 / 43.114 / 43.207 ms | 59.135 / 59.347 / 59.253 ms | 0.72 | PASS |
| 1 / 2 / 3 | u16 drop shadow 40 (factor 8) | 5 ms freeze after `pre` (1469) | 3.715 / 3.795 / 3.632 ms | 59.486 / 59.437 / 59.253 ms | 0.05 | PASS |
| 1 / 2 / 3 | 8-bit inner bevel 4 (factor 1) | `k` 8068 after `at_the_blur` (3480) | 0.037 / 0.043 / 0.032 ms | 2.467 / 2.503 / 2.468 ms | 0.00 | PASS |
| 4× parallel, 12 runs | 8-bit drop shadow 4 | — | 0.031–0.041 ms | 2.497–17.528 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | u16 drop shadow 4 | — | 0.032–0.048 ms | 2.497–17.528 ms | 0.00 | PASS 12/12 |
| 4× parallel, 12 runs | 8-bit drop shadow 40 | — | 42.159–44.431 ms | 58.913–71.195 ms | ≤ 0.73 | PASS 12/12 |
| 4× parallel, 12 runs | u16 drop shadow 40 | — | 3.550–4.819 ms | 58.535–62.937 ms | ≤ 0.06 | PASS 12/12 |
| 4× parallel, 12 runs | 8-bit inner bevel 4 | — | 0.031–0.037 ms | 2.452–2.597 ms | 0.00 | PASS 12/12 |

Cache integrity (byte, the same triggers): size 4 cancelled `k` 6047 after
the flip, returned 0.033 / 0.033 / 0.030 ms; size 40 on the 5 ms freeze,
returned 44.788 / 45.761 / 43.633 ms — every cancel inside the plane render
at progress 0 / stage 6, every re-render equal to the reference. The
13A-5a binary under the same rounds: mask blur factor 1 PASS 12/12 (0.032–
0.075 ms), byte guide build PASS 12/12 (0.009–0.026 ms), mask blur factor 8
PASS 11/12, **MISS 1/12** (70.962 ms vs 57.840 ms) — 13A-5a's known
factor-8 calibration MISS, owner 13A-5b-iii. No flake named in the known
list fired in `make test`.

## 13A-5b-iii-a verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-a/20260924-074106/`
(`cancel/` the four suites' runs 1, 2 and serial — plus the serial pass
before the warm (m2) timeline and the guide guard's serial re-runs —
`parallel/` and `parallel2/` the two 48-log sets of 4× load rounds with
their `uptime.log`, `guide-suites.log`, `make-test-final.log`,
`typecheck.log`, `make-app.log`, `logs/` the drives, one folder per drive,
`ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_byte_preparation_cancel --test regional_group_materialization_cancel --test regional_byte_primitive_cancel --test regional_style_preparation_cancel -- --nocapture` (runs 1, 2), then `--test-threads=1` (run 3) | 6 / 7 / 5 / 7 passed each run, 16 probe lines PASS each run (the first serial pass, before the warm (m2) timeline, failed the materialization probe — "the poller stalled past the stage: at return progress 1, stage 6" — see the decision above) |
| `cargo test --release --test regional_byte_guide --test regional_byte_nested_guide --test regional_byte_pass_through_guide --test regional_byte_clip_guide --test regional_byte_styled_clip_guide -- --nocapture` | 20 + 20 + 15 + 19 + 15 passed; every byte row PASS (the 13A-4a / 13A-4b-i tables print samples-after-cancel, all under 4096) |
| the four cancel binaries built once (`--no-run`); each BINARY run 4× concurrently for 3 rounds, twice (`uptime` before each round, set 1: preparation 13.5 / 28.1 / 29.8, primitive 32.3 / 32.5 / 32.9, materialization 30.8 / 30.5 / 31.2, style 31.3 / 42.1 / 65.7; set 2: 5.3 / 12.8 / 20.2, 26.8 / 31.0 / 32.3, 32.9 / 37.4 / 42.6, 42.8 / 47.3 / 52.0 — the machine was shared, a Blender render at ≈116 % CPU) | `regional_byte_primitive_cancel` 12/12 both sets; `regional_style_preparation_cancel` 10/12 then 12/12 (2 failures of the untouched `a_cancel_inside_the_plane_render_caches_no_partial_plane` at load 31–66: "returned before the entry" and a stall); `regional_group_materialization_cancel` 9/12 both sets ((m2) only, see below); `regional_byte_preparation_cancel` 3/12 then 7/12 (`the_byte_guide_blur_reports_inside_its_range` only, see below) |
| `the_byte_guide_blur_reports_inside_its_range` re-run serially by name after the load rounds (3×) | 3/3 passed |
| `make test` final | 2617 passed, 0 failed (264 binaries; no test added or removed — the guide guard renamed); no flake named in the known list fired |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase13a4_guide.py` / `verify_phase13a4b_nested_guide.py` / `verify_phase13a4bii_pass_through.py` / `verify_phase13a4biib_clip_members.py` / `verify_phase13a4biib2_styled_members.py` / `verify_phase12e2b_placement.py` | ALL GATES PASS each, exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |
| `wc -l core/src/regional.rs core/src/doc_regional.rs core/src/doc_regional_guide.rs core/tests/common/probe.rs` | 516 (502 before) / 352 (334) / 458 (454) / 465 (248) |

**Measured cancellation (the preparation suite, re-based).** DECLARED
before measuring (the contract deltas above). Runs 1 and 2 are the command
as written, run 3 `--test-threads=1`:

| Run | Probe | Calibration | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|---|
| 1 / 2 / 3 | mask blur, factor 1 (`k` half a pass after `at_the_blur`) | `c` 9.494 / 11.412 / 9.182 ns per cell | 0.035 / 0.037 / 0.035 ms | 1.039 / 1.047 / 1.038 ms | 0 | PASS |
| 1 / 2 / 3 | mask blur, factor 8 (freeze `min(5 ms, longest / 2)` = 5 ms) | timeline gap at `issued` 7280 = 50.242 / 48.965 / 47.462 ms; longest gap at 7280 each run (identification equal) | 40.293 / 41.310 / 41.033 ms | 51.242 / 49.965 / 48.462 ms | 0.78 / 0.82 / 0.84 | PASS |
| 1 / 2 / 3 | the byte guide build's grid pass (`k` = ⌈A/4096⌉/2 after stage 0's close) | `c` 2.020 / 2.024 / 2.074 ns per `R_0` sample (window 12.1 / 12.1 / 12.4 ms, stage 0's last report to the guide blur's first) | 0.017 / 0.017 / 0.015 ms | 1.008 ms | 0 | PASS (progress 0.395061 = close at return) |
| 4× parallel, 2 × 12 runs | mask blur, factor 1 | — | 0.015–0.090 ms | 1.015–1.154 ms | 0 | PASS 12/12, 12/12 |
| 4× parallel, 2 × 12 runs | mask blur, factor 8 | identification (`issued` = the longest gap's count, 7280) 12/12, 12/12 | 42.193–144.245 ms | 48.153–111.546 ms | ≤ 2.11 | PASS 10/12, **MISS 2/12** (144.245 vs 68.943 ms; 48.223 vs 48.198 ms); PASS 9/12, **MISS 3/12** (83.124 vs 71.047; 95.583 vs 80.511; 80.505 vs 71.615 ms) |
| 4× parallel, 2 × 12 runs | the grid pass | — | 0.009–0.030 ms | 1.008–1.015 ms | 0 | PASS 12/12, 12/12 |

The materialization suite's (m2) (g′) freeze probe, on the warm timeline:

| Run | `c_copy` (timeline gap at `issued`) | Longest gap (count) | Freeze | Cancel-to-return | Bound | Verdict |
|---|---|---|---|---|---|---|
| 1 | 2.217 ms at 1472 | 2.217 ms (1472) | 1.108 ms | 1.276 ms | 3.217 ms | PASS |
| 2 | 2.062 ms at 1472 | 2.062 ms (1472) | 1.031 ms | 0.997 ms | 3.062 ms | PASS |
| 3 | 3.844 ms at 1472 | 3.844 ms (1472) | 1.922 ms | 0.517 ms | 4.844 ms | PASS |
| 4× parallel, set 1 | — | identification equal 1/9 | — | 0.020–4.228 ms | 1.015–7.642 ms | PASS 8/9, **MISS 1/9** (2.722 ms vs 1.821 ms: issued at 1469, the timeline's longest gap a preemption at 12860); 3/12 runs failed the verdict (the freeze never fired inside the chain: "stalled … progress 1, stage 6" ×2, "progress 0, stage 6" ×1) |
| 4× parallel, set 2 | — | identification equal 0/9 | — | 0.028–7.718 ms | 1.004–16.991 ms | PASS 8/9, **MISS 1/9** (7.718 ms vs 3.068 ms: the longest gap at 3112); 3/12 runs failed the verdict ("stalled … progress 0.178 / 0.2245 / 1, stage 6") |

(m1), (m3), (m4) and the primitive and style suites' probes are unchanged
and PASSED every serial/parallel run (16 PASS lines per run).

**The guide guard under 4× load.** `the_byte_guide_blur_reports_inside_its_range`
failed 9/12 (set 1) and 5/12 (set 2) under 4× load and passed every serial
and parallel run and 3/3 serial re-runs by name. By clause: the
strictly-inside clause 1 + 1 ("scanned [0.604938]"), and — a signature the
spec expected only from a leak — the BOUNDS clause 8 + 4: the logger missed
the whole range and the first value it saw above stage 0's close was
already a final-stage row (0.605069–0.615868 > the advance 0.604938), which
starts and ends the scan outside `[close, guide]`. A logger stall therefore
fails `assert_ramp` falsely through the scan's start as well as through the
strictly-inside clause (open item below); the failures are recorded, not
chased.

The four byte `measure` tables' new `"the stage-0 fill"` rows
(`regional_byte_guide.rs`'s byte test moved to its u16 twin's shape
instead; the 13A-5a model,
bound `T_f + 4096·c`, `c` from the fill's own uncancelled window): the
pass-through 0.005 ms (c 2.36 ns, bound 1.010 ms) PASS; the clip unit 0.003
ms (c 2.34 ns, 1.010 ms) PASS; the styled clip "effects base" 0.004 ms (c
1.42 ns, 1.006 ms) PASS and "Deep knockout base" (D5, `f0 = 2A`, 11995904
samples) 0.004 ms (c 0.39 ns, 1.002 ms) PASS; the nested (13A-4b-i shape)
0.006 ms, ≈2686 samples after the cancel (declared 4096). Every other byte
row of the five tables re-based by `f0` and PASS.

## 13A-5b-iii-b-1 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-b-1/20260924-083126/`
(`make-test-1.log` / `make-test-2.log` with their `.uptime`,
`no-run.log`, `parallel/` and `parallel2/` the two 48-log sets of 4×
load rounds with their `uptime.log`, `style-cache-serial-reruns.log`,
`cancel-bench.log` with `cancel-bench.uptime`, `typecheck.log`,
`make-app.log`, `drive-*.log` and `12e2b/`, `12h/`, `12g/` the drives,
`ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 9.3 → 21.1) | 2612 passed, 0 failed, 7 ignored (264 binaries): the five `make cancel-bench` tests plus the two pre-existing ignored tests (`measured_accuracy_at_a_million_unknowns`, `fixed_24_48mp_corpus_retention`); 2612 + 5 = 13A-5b-iii-a's 2617, no test added or removed |
| `make test` run 2 (load 15.7 → 13.0) | 2612 passed, 0 failed, 7 ignored (the same seven) |
| the four cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 11.5 / 11.5 / 18.8, materialization 18.8 / 26.5 / 26.2, primitive 39.7 / 39.7 / 37.1, style 37.1 / 47.2 / 52.3; set 2: 50.0 / 50.0 / 46.6, 46.6 / 46.5 / 43.3, 44.8 / 44.8 / 42.0, 42.0 / 44.4 / 55.9) | preparation 12/12, 12/12; materialization 12/12, 12/12; primitive 12/12, 12/12; style 12/12, then **11/12**: set 2 round 3 (load 55.9), `a_cancel_inside_the_plane_render_caches_no_partial_plane` — "the poller stalled past the stage: returned before the entry" (`tests/common/probe.rs:93`), decision (c)'s residual, the open item below |
| that test re-run serially by name (3×, load 43.3) | 3/3 passed (drop shadow 4: entry 3480, cancel returned 0.032–0.042 ms later; drop shadow 40: entry 7810, 0.011–0.013 ms; progress 0 stage 6 at return; the re-render equals the reference each run) |
| `make cancel-bench` once, serially (load 37.1 → 29.3) | ALL FOUR binaries ran ("running 2 tests" preparation, "running 1 test" primitive, materialization, style); 5 passed, 0 failed, no panic; 16 probe lines, 16 PASS; the byte guide RAMP passed; exit 0 (no `make: ***` line) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

**The bench record (`make cancel-bench`, serial).** Bounds DECLARED
before measuring (the contract deltas of 13A-5a … 13A-5b-iii-a, unchanged):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| preparation | mask blur, factor 1 (feather 6) | 0.039 ms | T_f + W·c = 1.037 ms | 0 | PASS |
| preparation | mask blur, factor 8 (feather 40; timeline gap at `issued` 7280 = 46.896 ms, the longest gap at 7280 — identification equal; freeze 5 ms) | 40.958 ms | T_f + W·c = 47.896 ms | 0.85 | PASS |
| preparation | the byte guide build's grid pass | 0.014 ms | T_f + W·c = 1.008 ms | 0 | PASS |
| primitive | (q) 1/8, 3/8, 5/8, 7/8 of the chain | 0.016 / 0.034 / 0.011 / 0.016 ms | T_f + c_row = 1.008 ms (c_row 7.985 µs) | 0.00 | PASS ×4 |
| materialization | (g) the child row loop, N = 6 | 0.030 ms | T_f + c_row = 1.016 ms | 0.00 | PASS |
| materialization | (g′) the unpolled unit, freeze (c_copy = gap at `issued` 1472 = 2.287 ms, longest at 1472; freeze 1.144 ms) | 0.781 ms | T_f + c_copy = 3.287 ms | 0.00 | PASS |
| materialization | (s) a child's style plane render, size 4 | 0.035 ms | T_f + max(4096·c, c_map) = 2.523 ms | 0.00 | PASS |
| materialization | (c) a child's mask feather blur, feather 6 | 0.036 ms | T_f + 4096·c = 1.044 ms | 0.00 | PASS |
| style | 8-bit drop shadow 4 (factor 1) | 0.034 ms | T_f + max(4096·c, c_map) = 2.499 ms | 0.00 | PASS |
| style | u16 drop shadow 4 (factor 1) | 0.036 ms | T_f + max(4096·c, c_map) = 2.499 ms | 0.00 | PASS |
| style | 8-bit drop shadow 40 (factor 8, freeze 5 ms) | 43.204 ms | T_f + max(c_resample, c_map) = 60.349 ms | 0.71 | PASS |
| style | u16 drop shadow 40 (factor 8, freeze 5 ms) | 3.655 ms | T_f + max(c_resample, c_map) = 59.099 ms | 0.05 | PASS |
| style | 8-bit inner bevel 4 (factor 1) | 0.035 ms | T_f + max(t_style / ph, c_map) = 2.431 ms | 0.00 | PASS |
| preparation | `the_byte_guide_blur_reports_inside_its_range` (RAMP + PIN) | — | — | — | passed |

No MISS and no panic in the bench run. The known unrelated load flakes
(`precision_layer_blending`, `refinement_cutout`, `agent_tests`,
`phase8e_coverage`, `content_aware_*`, `metadata_tests`) did not fire in
either `make test` run.

## 13A-5b-iii-b-2 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-b-2/20260924-095134/`
(`make-test-1.log` with `make-test-1.uptime`, `make-test-2-attempt1-content-aware-flake.log`
and `make-test-2.log` with their `.uptime`, `no-run.log`, `parallel/` and
`parallel2/` the two 48-log sets of 4× load rounds with their `uptime.log` and
`summary.txt`, `style-cache-serial-reruns.log`, `cancel-bench.log` with
`cancel-bench.uptime`, `typecheck.log`, `make-app.log`, `drive-*.log` and
`12e2b/`, `12h/`, `12g/` the drives, `ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 20.1 just after) | 2613 passed, 0 failed, 9 ignored (264 binaries): 13A-5b-iii-b-1's 2612 + the new default `the_plan_counts_the_mask_blur`; ignored = the five bench tests, the two new bench RAMPs and the two pre-existing ignored tests |
| `make test` run 2, first attempt (load 19.2) | stopped at `content_aware_cost_tests::a_spanning_scratch_previews_far_faster_than_it_fills` ("preview 1.047815542s, fill 1.568124667s") — one of the known unrelated `content_aware_*` timing flakes; 232 passed before it, nothing regional |
| `make test` run 2 (load 18.0 → 10.1) | 2613 passed, 0 failed, 9 ignored (264 binaries) |
| the four cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 15.1 / 15.1 / 15.2, materialization 15.2 / 18.5 / 17.5, primitive 22.0 / 22.0 / 20.9, style 20.9 / 28.4 / 35.2; set 2: 39.5 / 39.6 / 39.6, 42.2 / 39.3 / 38.9, 36.6 / 34.3 / 34.3, 36.0 / 42.3 / 47.4) | preparation 12/12, 12/12; materialization 12/12, 12/12; primitive 12/12, 12/12; style 12/12, then **11/12**: set 2 round 1 (load 36.0), `a_cancel_inside_the_plane_render_caches_no_partial_plane` — "the poller stalled past the stage: at return progress 0.86476, stage 6, 15321 checkpoints (entry 15320 + k 0)": the poller missed the whole plane render and the advance after it, and its first observed `stage == 6` was a LOOP row (since `at_the_blur` no longer pins progress 0, a stage stall surfaces at the verdict instead of as "returned before the entry") — the residual row below, owner 13A-5b-iii-b-3 |
| that test re-run serially by name (3×, load 42.8 / 39.6 / 39.6) | 3/3 passed (drop shadow 4: entry 4947, cancel returned 0.030–0.036 ms later, progress 0.076331 stage 6; drop shadow 40: entry 9277, 0.011–0.013 ms, progress 0.07085 stage 6; the re-render equals the reference each run) |
| `make cancel-bench` once, serially (load 33.7 → 26.2) | ALL FOUR binaries ran ("running 3 tests" preparation, "running 1 test" primitive and materialization, "running 2 tests" style); 7 passed, 0 failed, no panic; 16 probe lines, 16 PASS; the byte guide RAMP and both new RAMPs passed; exit 0 |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

**The bench record (`make cancel-bench`, serial).** Bounds DECLARED
before measuring (the contract deltas of 13A-5a … 13A-5b-iii-a, unchanged;
the entries and verdicts re-based as above):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| preparation | mask blur, factor 1 (feather 6) | 0.038 ms | T_f + W·c = 1.036 ms | 0 | PASS |
| preparation | mask blur, factor 8 (feather 40; timeline gap at `issued` 8746 = 45.796 ms, the longest gap at 8746 — identification equal; freeze 5 ms) | 40.607 ms | T_f + W·c = 46.796 ms | 0.86 | PASS |
| preparation | the byte guide build's grid pass | 0.012 ms | T_f + W·c = 1.009 ms | 0 | PASS |
| primitive | (q) 1/8, 3/8, 5/8, 7/8 of the chain | 0.017 / 0.030 / 0.014 / 0.015 ms | T_f + c_row = 1.008 ms (c_row 8.060 µs) | 0.00 | PASS ×4 |
| materialization | (g) the child row loop, N = 6 | 0.030 ms | T_f + c_row = 1.016 ms | 0.00 | PASS |
| materialization | (g′) the unpolled unit, freeze (c_copy = gap at `issued` 2938 = 2.462 ms, longest at 2938; freeze 1.231 ms) | 0.657 ms | T_f + c_copy = 3.462 ms | 0.00 | PASS |
| materialization | (s) a child's style plane render, size 4 | 0.033 ms | T_f + max(4096·c, c_map) = 2.491 ms | 0.00 | PASS |
| materialization | (c) a child's mask feather blur, feather 6 | 0.035 ms | T_f + 4096·c = 1.043 ms | 0.00 | PASS |
| style | 8-bit drop shadow 4 (factor 1) | 0.036 ms | T_f + max(4096·c, c_map) = 2.574 ms | 0.00 | PASS |
| style | u16 drop shadow 4 (factor 1) | 0.040 ms | T_f + max(4096·c, c_map) = 2.574 ms | 0.00 | PASS |
| style | 8-bit drop shadow 40 (factor 8, freeze 5 ms) | 42.140 ms | T_f + max(c_resample, c_map) = 61.981 ms | 0.67 | PASS |
| style | u16 drop shadow 40 (factor 8, freeze 5 ms) | 3.724 ms | T_f + max(c_resample, c_map) = 59.285 ms | 0.05 | PASS |
| style | 8-bit inner bevel 4 (factor 1) | 0.038 ms | T_f + max(t_style / ph, c_map) = 2.459 ms | 0.00 | PASS |
| preparation | `the_byte_guide_blur_reports_inside_its_range` (RAMP + PIN) | — | — | — | passed |
| preparation | `the_mask_blur_reports_inside_its_range` (RAMP + PIN, feather 6 and 40; new) | — | — | — | passed |
| style | `the_byte_plane_render_reports_inside_its_range` (RAMP + PIN, drop shadow 4; new) | — | — | — | passed |

No MISS and no panic in the bench run. The at-return progress now sits
inside each stage's range (the mask blur's 0.0748 at factor 1, the (q)
chain's 0.0311 = its `pixels / work`, (g)'s 0.0383, the inner blurs' `m0`)
instead of 0. The known unrelated load flake
`content_aware_cost_tests::a_spanning_scratch_previews_far_faster_than_it_fills`
fired once (run 2's first attempt, above); no other known flake fired.

## 13A-5b-iii-b-3 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-b-3/20260924-103950/`
(`make-test-1.log` / `make-test-2.log` with their `.uptime`, `no-run.log`,
`parallel/` and `parallel2/` the two 48-log sets of 4× load rounds with their
`uptime.log` and `summary.txt`, `cancel-bench.log` with `cancel-bench.uptime`,
`latency.log` with `latency.uptime`, `latency-ab.log`, `typecheck.log`,
`make-app.log`, `drive-*.log` and `12e2b/`, `12h/`, `12g/` the drives,
`ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 15.7 before) | 2620 passed, 0 failed, 9 ignored: 13A-5b-iii-b-2's 2613 + the 7 new `job_control_park` tests (that binary: 7 passed in 0.11 s) |
| `make test` run 2 (load 95.0 before) | 2620 passed, 0 failed, 9 ignored; `job_control_park` 7 passed in 0.11 s; no known flake fired in either run |
| the four cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 30.6 / 29.6 / 29.6, materialization 36.8 / 38.8 / 40.3, primitive 50.8 / 50.8 / 48.7, style 48.7 / 47.9 / 53.5; set 2: 54.9 / 54.9 / 56.7, 53.9 / 49.9 / 53.4, 50.0 / 51.7 / 51.7, 50.8 / 64.7 / 68.1) | preparation 12/12, 12/12; materialization 12/12, 12/12; primitive 12/12, 12/12; style 12/12, 12/12 — 96/96, with NO "returned before the entry", "stalled past the stage" or "did not park" line in any log: the residual is gone |
| `make cancel-bench` once, serially (load 68.2 before) | ALL FOUR binaries ran (3 / 1 / 1 / 2 tests); 7 passed, 0 failed, no panic; 16 probe lines, 16 PASS; the three RAMP guards passed; exit 0 |
| `cargo run --release --example phase13a_regional_latency` (load 39.8) | see Measured latency below: equal to HEAD `057cbf5`'s build of the same example interleaved under the same load; slower than the 2026-09-23 table for the region and preview columns on BOTH builds (reported, not tuned) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

**The parked cache-integrity lines** (serial, by name, before the record):
preparation feather 6 parked at checkpoint 2939 (progress 0.016449, stage
6), feather 40 at 7165 (0.027859, 6); style drop shadow 4 at 4946
(0.076331, 6), 40 at 9276 (0.07085, 6); materialization (s) at 8959
(0.033038, 6), (c) at 3686 (0.013366, 6); each released 0.012–0.020 ms →
return, each re-render equal to the reference. Every count is the bench
probes' entry `n0` + 1 (+ `k` for (c)), so the report log and the polled
entry agree.

**The bench record (`make cancel-bench`, serial).** Bounds DECLARED before
measuring (unchanged; the bench probes were not re-based):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| preparation | mask blur, factor 1 (feather 6) | 0.034 ms | T_f + W·c = 1.036 ms | 0 | PASS |
| preparation | mask blur, factor 8 (feather 40; timeline gap at `issued` 8746 = 46.378 ms, the longest; freeze 5 ms) | 40.772 ms | T_f + W·c = 47.378 ms | 0.86 | PASS |
| preparation | the byte guide build's grid pass | 0.016 ms | T_f + W·c = 1.008 ms | 0 | PASS |
| primitive | (q) 1/8, 3/8, 5/8, 7/8 of the chain | 0.018 / 0.027 / 0.016 / 0.015 ms | T_f + c_row = 1.008 ms (c_row 7.902 µs) | 0.00 | PASS ×4 |
| materialization | (g) the child row loop, N = 6 | 0.028 ms | T_f + c_row = 1.016 ms | 0.00 | PASS |
| materialization | (g′) the unpolled unit, freeze (c_copy = gap at `issued` 2938 = 3.313 ms, the longest; freeze 1.656 ms) | 0.484 ms | T_f + c_copy = 4.313 ms | 0.00 | PASS |
| materialization | (s) a child's style plane render, size 4 | 0.036 ms | T_f + max(4096·c, c_map) = 2.422 ms | 0.00 | PASS |
| materialization | (c) a child's mask feather blur, feather 6 | 0.037 ms | T_f + 4096·c = 1.044 ms | 0.00 | PASS |
| style | 8-bit drop shadow 4 (factor 1) | 0.036 ms | T_f + max(4096·c, c_map) = 2.439 ms | 0.00 | PASS |
| style | u16 drop shadow 4 (factor 1) | 0.033 ms | T_f + max(4096·c, c_map) = 2.439 ms | 0.00 | PASS |
| style | 8-bit drop shadow 40 (factor 8, freeze 5 ms) | 42.914 ms | T_f + max(c_resample, c_map) = 58.622 ms | 0.73 | PASS |
| style | u16 drop shadow 40 (factor 8, freeze 5 ms) | 3.393 ms | T_f + max(c_resample, c_map) = 58.678 ms | 0.04 | PASS |
| style | 8-bit inner bevel 4 (factor 1) | 0.033 ms | T_f + max(t_style / ph, c_map) = 2.486 ms | 0.00 | PASS |
| preparation | `the_byte_guide_blur_reports_inside_its_range` (RAMP + PIN) | — | — | — | passed |
| preparation | `the_mask_blur_reports_inside_its_range` (RAMP + PIN) | — | — | — | passed |
| style | `the_byte_plane_render_reports_inside_its_range` (RAMP + PIN) | — | — | — | passed |

No MISS and no panic in the bench run.

## 13A-5b-iii-c-1 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-c-1/20260924-121951/`
(`make-test-1.log` / `make-test-2.log` with their `.uptime`, `no-run.log`,
`parallel/` and `parallel2/` the two 60-log sets of 4× load rounds with their
`uptime.log` and `summary.txt`, `cancel-bench.log` with `cancel-bench.uptime`,
`latency.log` with `latency.uptime`, `latency-ab.log` (loaded) and
`latency-ab-quiet.log`, `bisect/` (one log per bisected commit, `confirm/`
the second pass), `typecheck.log`, `make-app.log`, `drive-*.log` and
`12e2b/`, `12h/`, `12g/` the drives, `ui-probe/report.json`).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 23.9 before) | 2621 passed, 0 failed, 12 ignored (266 binaries): 13A-5b-iii-b-3's 2620 + the new default `the_typed_guide_leaks_no_report_after_its_advance` (the renamed typed INTERVAL guard is the same count); ignored = 13A-5b-iii-b-3's 9 + the three new bench tests |
| `make test` run 2 (load 46.0 before) | 2621 passed, 0 failed, 12 ignored; no known flake fired in either run |
| the FIVE cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 16.9 / 16.9 / 30.4, materialization 29.2 / 31.9 / 31.0, primitive 38.5 / 39.1 / 39.1, style 49.0 / 52.5 / 53.2, typed 49.1 / 49.1 / 49.1; set 2: 45.6 / 45.6 / 45.9, 42.8 / 39.8 / 37.8, 39.5 / 36.6 / 36.6, 34.5 / 38.7 / 43.6, 58.4 / 58.4 / 54.5) | 12/12 for every binary in both sets — 120/120; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 46.7 before) | ALL FIVE binaries ran (4 / 1 / 1 / 3 / 1 tests); 10 passed, 0 failed, no panic; 16 probe lines, 16 PASS; the six RAMP guards passed (the three earlier ones, the byte and typed guides' two-sub-range RAMPs and the typed plane RAMP); exit 0 |
| `cargo run --release --example phase13a_regional_latency` (load 28.4) + the A/B against `68650dd` (loaded 39.9 → 21.7, then QUIET 4.6 → 7.3) + the bisect | see Measured latency below: the region / preview slowdown HOLDS on a quiet machine (both rounds agree), bisected; recorded, not fixed (owner 13A-5b-iii-c-2) |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

No new drive: MCP `render` passes no job control, so nothing app-visible
moves (progress only).

**The bench record (`make cancel-bench`, serial).** Bounds DECLARED before
measuring and unchanged; the u16 probes use the re-based entry / exit /
verdict (contract deltas):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| preparation | mask blur, factor 1 (feather 6) | 0.036 ms | T_f + W·c = 1.036 ms | 0 | PASS |
| preparation | mask blur, factor 8 (feather 40; timeline gap at `issued` 8746 = 46.197 ms, the longest; freeze 5 ms) | 41.628 ms | T_f + W·c = 47.197 ms | 0.88 | PASS |
| preparation | the byte guide build's grid pass (at `close` 0.395061, stage 6 at return) | 0.013 ms | T_f + W·c = 1.009 ms | 0 | PASS |
| primitive | (q) 1/8, 3/8, 5/8, 7/8 of the chain | 0.020 / 0.028 / 0.012 / 0.013 ms | T_f + c_row = 1.008 ms (c_row 8.231 µs) | 0.00 | PASS ×4 |
| materialization | (g) the child row loop, N = 6 | 0.036 ms | T_f + c_row = 1.016 ms | 0.00 | PASS |
| materialization | (g′) the unpolled unit, freeze (c_copy = gap at `issued` 2935 = 0.756 ms; longest 2.024 ms at 2938; freeze 1.012 ms) | 0.723 ms | T_f + c_copy = 1.756 ms | 0.00 | PASS |
| materialization | (s) a child's style plane render, size 4 | 0.048 ms | T_f + max(4096·c, c_map) = 3.496 ms | 0.00 | PASS |
| materialization | (c) a child's mask feather blur, feather 6 | 0.036 ms | T_f + 4096·c = 1.043 ms | 0.00 | PASS |
| style | 8-bit drop shadow 4 (factor 1) | 0.036 ms | T_f + max(4096·c, c_map) = 2.497 ms | 0.00 | PASS |
| style | u16 drop shadow 4 (factor 1; entry 1484, k 6045; progress 0.46193 at return, inside `(pre, target]`; window T_0 12.8 → T_S 106.2 ms, t_style 93.5 ms) | 0.040 ms | T_f + max(4096·c, c_map) = 2.497 ms | 0.00 | PASS |
| style | 8-bit drop shadow 40 (factor 8, freeze 5 ms) | 43.624 ms | T_f + max(c_resample, c_map) = 60.075 ms | 0.72 | PASS |
| style | u16 drop shadow 40 (factor 8, freeze 5 ms; the entry is now the reduced plane's first row, so the freeze lands in the UP resample — progress 0.861221 = `target` at return, the verdict's inclusive end) | 45.005 ms | T_f + max(c_resample, c_map) = 62.054 ms | 0.72 | PASS |
| style | 8-bit inner bevel 4 (factor 1) | 0.039 ms | T_f + max(t_style / ph, c_map) = 2.625 ms | 0.00 | PASS |
| preparation | `the_byte_guide_blur_reports_inside_its_range` / `the_byte_guide_blurs_each_report_inside_their_sub_ranges` / `the_mask_blur_reports_inside_its_range` | — | — | — | passed |
| style | `the_byte_plane_render_reports_inside_its_range` / `the_typed_plane_render_reports_inside_its_range` | — | — | — | passed |
| typed | `the_typed_guide_blurs_each_report_inside_their_sub_ranges` | — | — | — | passed |

No MISS and no panic. The u16 size-40 latency moved from 13A-5b-iii-b-3's
3.393 ms to 45.005 ms because the probe's entry moved (the old entry, progress
reaching `pre`, fired before the plane render, so the 5 ms freeze caught the
first unpolled unit, the DOWN resample; the new one, the first value above
`pre`, fires in the reduced blur, so the freeze now catches the UP
resample, the larger declared unit) — the same unit the byte size-40 probe
measures (43.624 ms), inside the same bound.

## 13A-5b-iii-c-2 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5b-iii-c-2/20260924-140712/`
(`fmt.log`, `lint.log`, `make-test-1.log` / `make-test-2.log` with their
`.uptime`, `no-run.log`, `parallel/` and `parallel2/` the two 72-log sets of
4× load rounds with their `uptime.log` and `summary.txt`, `cancel-bench.log`
with `cancel-bench.uptime`, `typecheck.log`, `make-app.log` with
`make-app.uptime`, `drive-*.log` and `12e2b/`, `12h/`, `12g/` the drives,
`ui-probe/report.json`). The machine was heavily loaded throughout (1-minute
load 42–86, concurrent jobs not ours). The latency example was NOT re-run
(13A-latency's, deferred).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| the new default tests on the OLD sources (`git stash` of the three `src` files) | FAIL as intended: (i) "1 distinct reports strictly inside the child's composite range" and the parked test's span assertion; with only the Below `composite_contribution` reverted to `NONE`, (iii) fails at "2004 checkpoints … (≥ 4000)"; restored, all pass (4031 distinct values, 4004 checkpoints) |
| `make test` run 1 (load 47.9 before, 74.7 after) | 2624 passed, 0 failed, 14 ignored (267 binaries): 13A-5b-iii-c-1's 2621 + the three new default tests; ignored = 12 + the two new bench tests |
| `make test` run 2 (load 79.2 before, 45.1 after) | 2624 passed, 0 failed, 14 ignored; no known flake fired in either run |
| the SIX cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 42.8 / 42.4 / 42.4, materialization 44.0 / 44.7 / 43.8, primitive 43.7 / 43.7 / 52.0, style 48.6 / 63.9 / 74.2, typed guide 84.7 / 86.1 / 86.1, typed styled composite 83.4 / 83.4 / 83.4; set 2: 75.1 / 74.2 / 74.2, 71.8 / 71.4 / 65.1, 63.7 / 63.7 / 61.1, 56.9 / 61.4 / 68.1, 73.8 / 69.5 / 69.5, 69.5 / 67.5 / 67.5) | 12/12 for every binary in both sets — 144/144; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 64.0 before, 48.2 after) | ALL SIX binaries ran (4 / 1 / 1 / 3 / 1 / 2 tests); 12 passed, 0 failed, no panic; 17 probe lines, 17 PASS; the seven RAMP guards passed (13A-5b-iii-c-1's six and the new `the_materialized_composite_planes_report_inside_their_range`); exit 0 |
| `make typecheck` | 0 errors |
| `make app` | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

No new drive: MCP `render` passes no job control and no pixel moves; the
app's typed full-frame render under a job control reports more progress
points inside a styled layer's plane render (progress only).

**The new bench line (`make cancel-bench`, serial, load 64.0).** Bound
DECLARED before measuring (contract deltas):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| typed styled composite | u16 cancel inside the materialized styled composite, drop shadow 4 (entry checkpoint 15042, k 1001, issued at 16043 — inside the Below contribution's rows; progress 0.334408 = `e` at return; window 125.7–164.1 ms, t_comp 38.374 ms over n_comp 4005 checkpoints, c_row 9.582 µs) | 0.018 ms | T_f + c_row = 1.010 ms | 0.00 | PASS |

The sixteen earlier probe lines all PASS again (the preparation, primitive,
materialization and style suites' lines, `cancel-bench.log`), e.g. the u16
drop shadow 4 at 0.038 ms (bound 2.549 ms) and 40 at 44.954 ms (bound
61.510 ms).

## 13A-5c-1 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5c-1/20260924-151046/`
(`negative-control.log`, `fmt.log`, `lint.log`, `make-test-1.log` /
`make-test-2.log` with their `.uptime` (`make-test-2-flake.*` the aborted
first attempt), `no-run.log`, `parallel/` and `parallel2/` the two 84-log
sets of 4× load rounds with their `uptime.log` and `summary.txt`,
`cancel-bench.log` with `cancel-bench.uptime`, `typecheck.log`,
`make-app.log` with `make-app.uptime`, `drive-*.log` and `12e2b/`, `12h/`,
`12g/` the drives, `ui-probe/report.json`). The machine was heavily loaded
throughout (1-minute load 6–255, concurrent jobs not ours). The latency
example was NOT run (13A-latency's, deferred).

| Check | Result |
|---|---|
| the new default tests on the OLD admission / walk / `doc_group` sources (before those edits) | FAIL as intended, 3 of 4: the plan pin (`scratch_heap_bytes` 983040, not 1074724864), the count delta (0, not 8) and the parked cancel ("parked at checkpoint 8: progress 0.033333, stage 1 fails the verdict" — checkpoint 8 fell inside the accumulator fill); the exactness test passed (output unchanged by design); with the edits all four pass |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 30.2 before, 47.6 after) | 2628 passed, 0 failed, 15 ignored (268 binaries): 13A-5b-iii-c-2's 2624 + the four new default tests; ignored = 14 + the new bench test |
| `make test` run 2 | first attempt (load 47.0) stopped at the KNOWN flake `bayesian_matting::live_cancellation_stops_both_joint_work_and_fully_explicit_copy` ("operation completed before active cancellation witness"; not chased); re-run (load 39.4 before, 12.1 after): 2628 passed, 0 failed, 15 ignored |
| the SEVEN cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (`uptime` 1-minute load before each round, set 1: preparation 14.7 / 25.7 / 36.5, materialization 46.2 / 80.8 / 120.6, primitive 162.1 / 172.2 / 188.8, style 188.8 / 213.0 / 239.8, typed guide 251.7 / 255.3 / 255.3, typed styled composite 246.3 ×3, context preparation 233.7 ×3; set 2: 215.1 / 208.3 / 192.4, 192.4 / 175.8 / 158.5, 146.9 / 137.9 / 137.9, 127.3 / 126.5 / 124.5, 121.6 ×3, 112.5 / 112.5 / 104.3, 104.3 ×3) | 12/12 for every binary in both sets — 168/168; no "returned before the entry", "stalled past the stage" or "did not park" line; the new parked cancel parked at checkpoint 8 (progress 0, stage 1) in all 24, release-to-return 0.014–6.564 ms (printed only) |
| `make cancel-bench` once, serially (load 88.5 before, 64.7 after) | ALL SEVEN binaries ran (4 / 1 / 1 / 1 / 3 / 1 / 2 tests); 13 passed, 0 failed, no panic; 18 probe lines, 18 PASS; exit 0 |
| `make typecheck` | 0 errors |
| `make app` (load 6.5 before, 19.5 after) | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 |

No new drive: MCP `render` passes no job control and does not expose the
plan (open item, 13B), and no pixel moves.

**The new bench line (`make cancel-bench`, serial, load 88.5).** Bound
DECLARED before measuring (contract deltas):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| context preparation | byte cancel inside layer 1's description parse (4 MiB meta, unmodelled profile; entry checkpoint 7, freeze 5.000 ms, issued at 7, returned at 8; progress 0, stage 1 at return; c_parse 24.681 ms, the timeline's gap at 7) | 19.814 ms | T_f + c_parse = 25.681 ms | 0.76 | PASS |

The seventeen earlier probe lines all PASS again (`cancel-bench.log`).

## 13A-5c-2a verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5c-2a/20260924-161414/`
(`negative-control.log`, `fmt.log`, `lint.log`, `make-test-1.log` /
`make-test-2.log` with their `uptime` lines first and last, `no-run.log`,
`parallel/` and `parallel2/` the two 96-log sets of 4× load rounds with
their `uptime.log` and `summary.txt`, `cancel-bench.log` with
`cancel-bench.uptime`, `typecheck.log`, `make-app.log` with
`make-app.uptime`, `drive-*.log` and `12e2b/`, `12h/`, `12g/` the drives,
`ui-probe/report.json`). The machine was loaded throughout (1-minute load
7–81, concurrent jobs not ours). The latency example was NOT run
(13A-latency's, deferred).

| Check | Result |
|---|---|
| the new default tests on HEAD's `inpaint_source.rs` and `precision_conversion.rs` (the `doc.rs` edit standing) | FAIL as intended, 4 of 7: test 1's limit witness ("(a) refused under the tiled limit 6083792 (T 3513072): operation needs up to 11 MiB …"), the parked blur cancel ("the entry never came": no stage-6 report under a NONE job), and the parked chain / tile-loop cancels ("the fallback polled only 5 times", "the tile loop polled only 13 times"); the exactness, fallback-exactness and cut tests passed (output unchanged by design); with the edits all seven pass |
| `precision_conversion`'s new unmodelled-profile test with the plan's context term zeroed, the `inpaint_source.rs` edits standing (the objection-1 scenario) | FAIL as intended: the plan pin (`scratch_bytes ≥ 64·L + 256·L + 4 MiB`), and with that assert disabled the render itself ("operation needs up to 261 MiB of working memory; 68 MiB is available under the working-memory limit"); with both edits it passes |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 23.7 before, 68.2 after) | 2636 passed, 0 failed, 17 ignored (269 binaries): 13A-5c-1's 2628 + the seven new default tests + the new `precision_conversion` test; ignored = 15 + the two new bench tests |
| `make test` run 2 (load 68.2 before, 19.1 after) | 2636 passed, 0 failed, 17 ignored; no known flake hit in either run |
| the EIGHT cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 14.8–30.0 in set 1, 26.5–80.9 in set 2) | 12/12 for every binary in both sets — 192/192; no "returned before the entry", "stalled past the stage" or "did not park" line; the new suite's parked cancels parked at checkpoints 173 (blur; progress 0, stage 6), 463 (chain) and 497 (tile loop) in all 24, release-to-return 0.007–0.088 ms (printed only) |
| `make cancel-bench` once, serially (load 60.6 before, 39.3 after) | ALL EIGHT binaries ran (4 / 1 / 1 / 1 / 2 / 3 / 1 / 2 tests); 15 passed, 0 failed, no panic; 20 probe lines, 20 PASS; exit 0 |
| `job_control::sample_all_preparation_cancellation_on_plain_48mp_and_grouped_24mp` (the grouped fallback now returns per row; printed, load 40.1) | 8000×6000 plain 0.042 ms, 6000×4000 grouped 43.6 ms cancel-to-return (< 2 s asserted) |
| `make typecheck` | 0 errors |
| `make app` (load 7.3 before, 10.2 after) | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | 56 groups passed (+ pass 2: 1), exit 0 (no lock-state failure) |

No new drive: no tool, argument or pixel changes.

**The two new bench lines (`make cancel-bench`, serial, load 60.6).** Bounds
DECLARED before measuring (contract deltas):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| sampling fallback | byte cancel inside the full-frame fallback (3000×2000, an isolated group of 6 canvas-sized rasters; entry checkpoint 10021, k 1, issued at 10022, returned at 10023; progress 0, stage 1 at return; T(1) 114.6 ms, T(6) 283.3 ms, c_row 0.0169 ms) | 0.028 ms | T_f + c_row = 1.017 ms | 0.00 | PASS |
| sampling fallback | byte cancel inside the tile loop (3000×2000, 6 canvas-sized rasters under aligned masks; entry checkpoint 22442, k 1, issued at 22443, returned at 22444; progress 0, stage 1 at return; T(1) 74.7 ms, T(6) 231.8 ms, c_tile_row 0.0052 ms) | 0.011 ms | T_f + c_tile_row = 1.005 ms | 0.00 | PASS |

The eighteen earlier probe lines all PASS again (`cancel-bench.log`).

## 13A-5c-2b verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5c-2b/20260924-173035/`
(`negative-control.log`, `fmt.log`, `lint.log`, `make-test-1.log` /
`make-test-2.log` with their `uptime` lines first and last, `no-run.log`,
`binaries.txt`, `parallel/` and `parallel2/` the two 108-log sets of 4× load
rounds with their `uptime.log` and `summary.txt`, `cancel-bench.log` with
`cancel-bench.uptime` and `cancel-bench-materialization-rerun.log`,
`typecheck.log`, `make-app.log` with `make-app.uptime`, `drive-*.log` and
`12e2b/`, `12h/`, `12g/` the drives, `drive-9h-attempt1*.log`,
`lock-state-attempt1.txt`). The machine was loaded throughout (1-minute load
8–90, concurrent jobs not ours). The latency example was NOT run
(13A-latency's, deferred).

| Check | Result |
|---|---|
| the new suite with `plate_arm`'s `Tree` mapped to `FullFrame` | FAIL as intended, 3 of 5: the route witness ("(a) refused under the route limit 8041792: operation needs up to 45 MiB …", after (a)'s exactness passed), the parked preparation cancel (its entry calibration refused under `L`: "operation needs up to 84 MiB …") and the parked tile-loop cancel ("the worker did not park at 463: it returned first"); the cut and promotion tests passed (output unchanged by design); restored, all five pass |
| the promotion pin with the plan arm's `(20·tile − 16n)⁺` term removed | FAIL as intended at (t1) ("operation needs up to 5 MiB of working memory; 4 MiB is available …"); (t2) alone PASSES under the same edit — the group's `56n·depth` projection term covers the tree's `20·tile`, so (t2) pins the plan without witnessing the term (the spec expected both to fail) |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `make test` run 1 (load 33.0 before, 33.3 after) | 2641 passed, 0 failed, 18 ignored: 13A-5c-2a's 2636 + the five new default tests; ignored = 17 + the new bench |
| `make test` run 2 (load 29.7 before, 10.0 after) | 2641 passed, 0 failed, 18 ignored |
| the NINE cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 16.2–42.3 in set 1, 42.3–90.4 in set 2) | 12/12 for every binary in both sets — 216/216; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 60.7 before, 38.8 after) | ALL NINE binaries ran; 15 passed, 1 FAILED: the untouched `regional_group_materialization_cancel::a_concurrently_cancelled_materialization_returns_within_the_model` printed its first probe line (PASS) and then failed "the poller stalled past the stage: at return progress 1, stage 6, 36549 checkpoints (entry 2930 + k 0)" — the spinning poller's documented load residual (it renders through `render_region`, `Cut::NONE`); 18 probe lines printed, 18 PASS, 0 MISS. That binary re-run once serially (load 31.0 → 29.2): 1 passed, its 4 probe lines PASS |
| `make typecheck` | 0 errors |
| `make app` (load 27.2 before, 22.2 after) | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 1: `current-layer-commands-ui` FAILED on activation ("The owned editor never became the real key window (active=false, visible=true, windowKey=false, keyWindow=nil)"), every other probe PASS (55); the session was LOCKED (`ioreg`: `CGSSessionScreenIsLocked` true at 17:59) |

No new drive: no tool, argument or pixel changes.

**The new bench line (`make cancel-bench`, serial, load ≈42).** Bound
DECLARED before measuring (contract deltas):

| Suite | Probe | Cancel-to-return | Bound | Past the floor | Verdict |
|---|---|---|---|---|---|
| sampling tree | byte cancel inside the tree plate's tile loop (3000×2000, an isolated group of 6 canvas-sized rasters; entry checkpoint 6060, k 1, issued at 6061, returned at 6062; progress 0, stage 1 at return; T(1) 429.5 ms, T(6) 965.2 ms, c_tree_row 0.1647 ms) | 0.034 ms | T_f + c_tree_row = 1.165 ms | 0.00 | PASS |

Open item (ii): the tree plate's T(1) / T(6) = 429.5 / 965.2 ms against
the fallback's 114.6 / 283.3 ms recorded by 13A-5c-2a (load 60.6), and
against the fallback's own 400.9 / 564.6 ms in this same bench run (load
≈40, the fallback bench now wraps the group in a guided Shadows/Highlights,
so its figures are not the 13A-5c-2a fixture's): 3.7× / 3.4× the 13A-5c-2a
figures at unequal load — owner 13A-latency, to be re-measured at equal
load.

## 13A-5c-3 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-5c-3/20260924-185528/`
(`negative-control.log`, `fmt.log`, `lint.log`, `new-suite.log`,
`make-test-1.log` / `make-test-2.log` with their `uptime` lines first and
last, `no-run.log`, `binaries.txt`, `parallel/` and `parallel2/` the two
108-log sets of 4× load rounds with their `uptime.log` and `summary.txt`,
`cancel-bench.log` with `cancel-bench.uptime`, `typecheck.log`,
`make-app.log` with `make-app.uptime`, `drive-*.log` and `12e2b/`, `12h/`,
`12g/` the drives, `drive-9h.log` / `drive-9h-run.log` / `9h.json`,
`lock-state.txt`). The machine was loaded throughout (1-minute load
30–103, concurrent jobs not ours). The latency example was NOT run
(13A-latency's, deferred).

| Check | Result |
|---|---|
| the new suite with `regional_admission` reverted to `context_scratch` and without `+ uncharged` | FAIL as intended, 16 of 17; only the plain-raster control (no metadata, 0 uncharged) passes; restored, all 17 pass |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test regional_resource_accounting` | 17 passed |
| `make test` run 1 (load 68.7 before, 103.2 after) | 2658 passed, 0 failed, 18 ignored: 13A-5c-2b's 2641 + the new suite's 17 |
| `make test` run 2 (load 96.3 before, 42.8 after) | 2658 passed, 0 failed, 18 ignored |
| the NINE cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 30.0–50.2 in set 1, 39.2–75.9 in set 2) | 12/12 for every binary in both sets — 216/216; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 60.7 before, 36.3 after) | ALL NINE binaries ran; 16 passed, 0 failed; 21 probe lines printed, 21 PASS, 0 MISS — `a_concurrently_cancelled_materialization_returns_within_the_model` passed this run (the 13A-5c-2b load residual did not recur) |
| `make typecheck` | 0 errors |
| `make app` (load 8.8 before, 29.1 after) | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 1: `current-layer-commands-ui` FAILED on activation ("The owned editor never became the real key window (active=false, visible=true, windowKey=false, keyWindow=nil)"), every other probe PASS (55); the session was LOCKED (`ioreg`: `CGSSessionScreenIsLocked` true at 19:16) — 13A-5c-2b's same attempt-1 outcome |

No new drive: no tool, argument or pixel change, and the app reads no plan
scratch figure.

## 13A-6a verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-6a/20260924-203249/`
(`negative-control.log`, `edt-f32-control.log`, `fmt.log`, `lint.log`,
`new-suite.log`, `pins.log`, `make-test-1.log` / `make-test-2.log` with their
`uptime` lines first and last, `no-run.log`, `binaries.txt`, `parallel/` and
`parallel2/` the two 120-log sets of 4× load rounds with their `uptime.log`
and `summary.txt`, `cancel-bench.log` with `cancel-bench.uptime`,
`typecheck.log`, `make-app.log` with `make-app.uptime`, `drive-13a6a.log` and
`13a6a/phase13a6a.json` the new drive, `drive-*.log` and `12e2b_placement/`,
`12h_conversion/`, `12g_clipboard/` the regression drives, `drive-9h.log` /
`9h.json`). The machine was loaded by concurrent jobs not ours (1-minute load
4.8–80.9). The latency example was NOT run (13A-latency's, deferred).

| Check | Result |
|---|---|
| the new suite with `apply` routed to the whole-plane path (the canvas work check + `spatial_kernel` over the canvas; Refine behind the canvas sum) | FAIL as intended, 5 of 12: the three beyond-cap tests, the 4400² far-support test (its Feather/Smooth 64 cases are beyond the cap there) and the parked cancel (its stage-6 reports count the canvas's rows, not the window's); restored, all 12 pass |
| the far-support and envelope pins on the pre-13A-6a f32 `edt_1d` | FAIL as intended, 2 of 2 (`grow 0.5` on the 9000×8 pin: first difference at x = 608 of the translated canvas; `grow 1` on 4400²: at (4218, 4202)); restored, both pass |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test bounded_selection_support` | 12 passed, 1 ignored (the bench probe) |
| `cargo test --release --test precision_scalar_selection --test precision_selection_refine --test precision_channel_math --test regional_resource_accounting` | 16 + 6 + 8 + 18 passed (the existing selection pins unchanged; the accounting suite's new pin and three absolute twin figures) |
| `make test` run 1 (load 9.97 before, 23.70 after) | 2671 passed, 0 failed, 19 ignored: 13A-5c-3's 2658 + the new suite's 12 + the accounting pin |
| `make test` run 2 (load 23.70 before, 13.86 after) | 2671 passed, 0 failed, 19 ignored |
| the TEN cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 9.4–65.6 in set 1, 31.0–80.9 in set 2) | 12/12 for every binary in both sets — 240/240; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 48.1 before, 28.0 after) | ALL TEN binaries ran; 17 passed, 0 failed; 22 probe lines, 22 PASS, 0 MISS. The new probe: a cancel inside the crop's horizontal pass (feather 250, B1's 600×600 window) returned in 0.840 ms (issued at checkpoint 786, entry 696 + k 90, stage 6, progress 0.0258); `c_cell` 0.4405 µs over 599 timed rows; bound `T_f + 4096·c_cell` = 2.804 ms — PASS |
| `make typecheck` | 0 errors |
| `make app` (load 23.8 before, 11.3 after) | BUILD SUCCEEDED |
| `scripts/verify_phase13a6a_bounded_selection.py` | B1–B4 PASS, exit 0: B1 feather 250 on 16-bit 2048×1024 succeeds (selection bounds 600×600 at (250, 150) = the window), `get_document` 2048×1024; B2 the 64×64 grid over the window equal to the 1024×1024 twin at all 4096 points (3823 nonzero, centre 15904), 0 at the nine outside points on both; B3 smooth 250 equal (2305 nonzero, centre 9706), 0 outside; B4 the 8-bit byte feather 250 succeeds, `coverage_storage` "u8" |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 0: 56 groups + pass 2 passed, `failures` empty — `current-layer-commands-ui` PASSED this run (the session was unlocked) |

`pgrep -f '^\./build/.*MacOS/Rasterize'` was empty afterwards and no
`scripts/**/__pycache__` was left.

## 13A-6b verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-6b/20260924-213947/`
(`negative-control-A.log`, `negative-control-B.log`, `fmt.log`, `lint.log`,
`targeted-suites.log`, `parked.log`, `make-test-1.log` / `make-test-2.log`
with their `uptime` lines first and last, `no-run.log`, `binaries.txt`,
`parallel/` and `parallel2/` the two 132-log sets of 4× load rounds with their
`uptime.log` and `summary.txt`, `cancel-bench.log` with
`cancel-bench.uptime`, `cancel-bench-run1-harness-scan.log` with its
`.uptime` (the first run, below), `typecheck.log`, `make-app.log` with
`make-app.uptime`, `drive-13a6b.log` / `drive-13a6b.uptime` and
`13a6b/phase13a6b.json` the new drive, `drive-*.log` and `12e2b_placement/`,
`12h_conversion/`, `12g_clipboard/` the regression drives, `drive-9h.log` /
`9h.json`). The machine was loaded by concurrent jobs not ours (1-minute load
4.7–72.9). The latency example was NOT run (13A-latency's, deferred).

| Check | Result |
|---|---|
| negative control A: `rasterize` charging `n + E·H` in place of `n` AND `first = 0, last = H` (the pre-13A-6b charge and row range; cursor and polling kept) | FAIL as intended, exactly 3 of 10: the beyond-cap field (the sweep refused with the sentence), the reports pin (8,000 stage-5 reports, not 102) and the boundary (the sweep refused what the full scan accepts, 256,015,998 > cap); all else green; restored |
| negative control B: `charge(n + E)` in place of `charge(n)` | FAIL as intended, exactly 1 of 10: the boundary monotonicity test alone; restored |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test bounded_artwork_coverage --test phase8e_coverage --test bounded_selection_support` | 10 passed + 1 ignored (the bench probe); `phase8e_coverage` 18 passed, unchanged; 12 passed + 1 ignored. The boundary test's two ≈128M-iteration pair loops ran in ≈0.5 s together here (the spec's ≈2–3 s estimate was conservative). The parked cancel: `n0` 14, parked at checkpoint 55 (progress 0.0045, stage 5), released 0.058 ms → return, then a complete run equal to the reference |
| `make test` run 1 (load 34.03 before, 21.89 after) | 2681 passed, 0 failed, 20 ignored: 13A-6a's 2671 + the new suite's 10 (and its bench probe ignored) |
| `make test` run 2 (load 14.48 before, 7.48 after) | 2681 passed, 0 failed, 20 ignored |
| the ELEVEN cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 9.7–72.9 in set 1, 41.6–67.2 in set 2) | 12/12 for every binary in both sets — 264/264; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench`, first run (load 37.01 before, 20.08 after) | ALL ELEVEN binaries ran; 18 passed, 0 failed; 23 probe lines, 22 PASS, **1 MISS — the new probe: 12.130 ms** against the bound 1.008 ms, with the worker returning ONE checkpoint after the cancel (issued at 2515, 2516 at return). The bench's render was `product_with`, whose post-refusal `0x4a4a` scan of the 65 MB output (and the output's deallocation) ran INSIDE the probe's cancel-to-return interval — the harness's own O(n) work, not the sweep's. Recorded, not widened: the bench now calls the FFI bare into a caller-owned output (`bare_edit`) and checks the sentinel after the probe returns; the bound is unchanged |
| `make cancel-bench`, recorded run (load 14.32 before, 9.76 after) | ALL ELEVEN binaries ran; 18 passed, 0 failed; 23 probe lines, 23 PASS, 0 MISS. The new probe: a cancel inside the row loop (4096×8000, 2 active edges per row) returned in 0.003 ms (issued at checkpoint 2515, entry 2015 + k 500, stage 5, progress 0.062625); `c_row` 7.9846 µs over 3600 timed rows; bound `T_f + c_row` = 1.008 ms — PASS |
| `make typecheck` | 0 errors |
| `make app` (load 4.73 before, 15.29 after) | BUILD SUCCEEDED |
| `scripts/verify_phase13a6b_bounded_artwork.py` (load 14.07 before, 11.25 after) | B1–B3 PASS, exit 0: B1 the 40-line 16-bit 240×16400 text layer adds with no "work limit"/"edge limit", `get_document` 240×16400, the layer 43×16017 at (32, 92) (≥ 15,800), ink found at x 51 on lines 1, 20 and 40 (3 non-white 5×5 samples each, e.g. rows 460 / 476 of line 1's box); B2 the 8-bit twin identical (43×16017, 3 per line); B3 the 240×2600 six-line control 43×2417, ink on lines 1, 3 and 6. The pre-13A-6b app was not rebuilt: the beyond-cap proof is the suite's |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS (12 PASS), exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 0: 56 groups + pass 2 passed, `passed` true |

`pgrep -f '^\./build/.*MacOS/Rasterize'` was empty afterwards and no
`scripts/**/__pycache__` was left.

## 13A-6c-1 verification record (2026-09-24)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-6c-1/20260924-225511/`
(`negative-control-i.log`, `negative-control-ii.log`, `fmt.log`,
`lint.log`, `targeted-suites.log`, `make-test-1.log` / `make-test-2.log`
with their `uptime` lines first and last, `no-run.log`, `binaries.txt`,
`parallel/` and `parallel2/` the two 146-file sets of 4× load rounds (144
logs, `uptime.log`, `summary.txt`), `cancel-bench.log` with
`cancel-bench.uptime`, `cancel-bench-materialization-rerun.log`,
`typecheck.log`, `make-app.log` with `make-app.uptime`, `drive-13a6c1.log`
/ `drive-13a6c1.uptime` and `13a6c1/phase13a6c1.json` the new drive,
`drive-13a6b.log` and `13a6b/` (carried nit 2), `drive-*.log` and
`12e2b_placement/`, `12h_conversion/`, `12g_clipboard/` the regression
drives, `drive-9h.log` / `9h.json`). The machine was loaded by concurrent
jobs not ours (1-minute load 9.1–64.2). The latency example was NOT run
(13A-latency's, deferred).

| Check | Result |
|---|---|
| negative control (i): `plan` charging the pre-13A-6c-1 figures (`2n + Σ box·(64 \| 1)` over every dab, scratch `4 MiB + 12n + 8·len`) with the new rasterizer kept | FAIL as intended, exactly 3 of 17: the beyond-cap test (the product refused with the 256M sentence), the scratch pin (4,243,488 ≠ 4,203,496 on the one-dab 64×64 fixture) and the nothing-to-draw pin (`work_units` 8,192 ≠ 4,096 on the empty list); all else green; restored |
| negative control (ii): the EXTERIOR test without condition (a) (`contains \|\| !contains`) | FAIL as intended, 3 of 17: `hard_needle_inside_one_pixel_is_band` (first difference at (10, 10): 0 vs 6144), `small_hard_dabs_equal_the_reference` (hard 1 at a pixel centre, roundness 0.01, 45°: (3, 3) 0 vs 6144) and `pixel_mode_and_soft_dabs_equal_the_reference` (its hard 0.37/30°/opacity-0.6 brush, diameter 1 at a pixel centre: (40, 40) 0 vs 11059). `hard_sub_pixel_dab_inside_one_pixel_is_band` did NOT fail: a 0.6-px hard dab is never classified (`d[2] < 1` runs `evaluate` on its box, objection 1's author correction), so condition (a) cannot reach it — it pins that bypass instead; restored |
| `cd core && cargo fmt --check && cd .. && make lint` | clean |
| `cargo test --release --test bounded_brush_footprint --test precision_brush_tools --test bounded_artwork_coverage --test phase8e_coverage` | 17 passed + 1 ignored (the bench probe); `precision_brush_tools` 16 passed, unchanged and untouched; 10 passed + 1 ignored; 18 passed. The parked cancel: `n0` 79, parked at checkpoint 120 (progress 0.15, stage 3), released 0.051 ms → return, then a complete run equal to the reference |
| `make test` run 1 (load 64.17 before, 35.48 after) | 2698 passed, 0 failed, 21 ignored: 13A-6b's 2681 + the new suite's 17 (and its bench probe ignored) |
| `make test` run 2 (load 35.48 before, 20.33 after) | 2698 passed, 0 failed, 21 ignored |
| the TWELVE cancel binaries built once (`--no-run`); each BINARY run 4× concurrently (default tests only) for 3 rounds, twice (1-minute load 19.7–32.1 in set 1, 21.0–41.6 in set 2) | 12/12 for every binary in both sets — 288/288; no "returned before the entry", "stalled past the stage" or "did not park" line |
| `make cancel-bench` once, serially (load 36.41 before, 32.49 after) | ALL TWELVE binaries ran; 18 passed, **1 failed**; 21 probe lines, 21 PASS, 0 MISS. The failure is the pre-existing 13A-5b-ii-a freeze probe (`regional_group_materialization_cancel`, (g′)): "the poller stalled past the stage: at return progress 1, stage 6, 36549 checkpoints (entry 2930 + k 0)" — a harness stall under load, in code 13A-6c-1 does not touch; that binary re-run alone at load 27.65 passed with all four probe lines (`cancel-bench-materialization-rerun.log`). Recorded, not chased. The new probe: a cancel inside the dab loop (1600×1600, 120 hard 512-px dabs, `E_dab` 395,016) returned in 0.016 ms (issued at checkpoint 4454, entry 3954 + k 500, stage 3, progress 0.0525); `c_eval` 11.645 ns over 59.0 timed dabs; bound `T_f + 64·513·c_eval` = 1.382 ms — PASS |
| `make typecheck` | 0 errors |
| `make app` (load 22.82 before, 9.36 after) | BUILD SUCCEEDED |
| `scripts/verify_phase13a6c_bounded_strokes.py` (load 9.09) | B1–B4 PASS, exit 0: B1 40 dabs (old charge ≥ 674,928,640, new ≤ 26,063,400), reply "Native version 1 dab coverage; source RGB stays u16." with no "work units", `get_document` 1600×1200, (800,600) and (800,350) `[0, 0, 0]`, (800,200) and (43,600) `[65535, 65535, 65535]`; B2 2 dabs (old ≥ 37,394,432), black at (650,600)/(400,400), white at (650,200)/(100,600); B3 the 8-bit twin ("RZBR v1 byte dabs", 40 dabs), the same samples at 0/255; B4 541 dabs, Σ ≥ 300,016,960, refused with "native brush exceeds 256 million footprint work units", (800,600) and (300,300) still white, 1600×1200 |
| `scripts/verify_phase13a6b_bounded_artwork.py` (carried nit 2: B3 now asserts layer height ≥ 2,400) | B1–B3 PASS, exit 0; B3's layer 43×2417 |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS, exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 0: 56 groups + pass 2 passed, `passed` true |

`pgrep -f '^\./build/.*MacOS/Rasterize'` was empty afterwards and no
`scripts/**/__pycache__` was left.

## 13A-6c-2 verification record (2026-09-25)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-6c-2/`
(`phase13a6c2.json` the new drive with its `mcp_url`; `logs/` —
`targeted-suites.log`, `lint.log`, `make-test-1.log` / `make-test-2.log`,
`typecheck.log`, `make-app.log` / `make-app.uptime`, `drive-*.log` /
`drive-*.uptime`, `drive-9h-run.log` / `drive-9h-rerun-run.log` with
`drive-9h.lockstate`, `cancel-bench.log` / `cancel-bench.uptime`,
`mcp-reference.log`, `exits.txt`), and the regression receipts beside it
(`13a-6c-2-regress-6c1/`, `-12e2b/`, `-12h/`, `-12g/`, `-9h/`,
`-9h-rerun/`). The machine was loaded by concurrent jobs not ours (1-minute
load 8.6–57.9). The latency example was NOT run (13A-latency's, deferred).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check && cargo test --release --test bounded_brush_footprint && cargo test --release --test precision_brush_tools` | clean; 18 passed + 1 ignored (13A-6c-1's 17 + `plan_checks_the_dab_count_before_the_footprint`); 16 passed |
| `make lint` | clean |
| `make test` run 1 / run 2 (load 14.24 after run 2) | 2699 passed, 0 failed, 21 ignored, both runs (13A-6c-1's 2698 + the new pin) |
| `make typecheck` | 0 errors |
| `make app` (load 8.56 before, 16.13 after) | BUILD SUCCEEDED |
| `scripts/verify_phase13a6c2_stroke_refusals.py` (load 16.76 before, 25.51 after) | every gate PASS, exit 0. T1: reply `coverage` "Native version 1 dab coverage generated from points; source RGB stays u16.", (800,600) `[0,0,0]`, (800,400) white; the `stroke_version: 1` twin's `mean_rgba_codes` EQUAL at all six points ((269,600) black, (800,568) 20480, (800,600) black, (800,632) white, (1331,600) white, (500,610) black). T2: a 56,001-dab walk refused with "This stroke exceeds the native brush limit of 50,000 dabs; no partial stroke was applied. Use a shorter stroke or wider spacing.", three points white, 1600×1200. T3: "generated from points", alpha 0 at (800,600), 65535 at (800,400). T4: 801 dabs, Σ ≥ 380,225,088, refused with "native brush exceeds 256 million footprint work units" (no "50,000"), white, 1600×1200. T4b: the same sentence on the mask target, alpha 65535. B1 / B2 (`dodge_burn`, `spot_heal_stroke`): the dab sentence at 8 bits, white unchanged. B3: black (800,600), white (800,400). Q1: the dab sentence, `get_selection` unchanged. C1: the 2,133,376-byte base64 body reached the tool (no transport rejection) and refused with the dab sentence; white |
| `scripts/verify_phase13a6c_bounded_strokes.py` (load 38.36 / 42.74; carried nit 3: the receipt now records `mcp_url`) | B1–B4 PASS, exit 0 (B4 541 dabs, the 256M sentence) |
| `scripts/verify_phase12e2b_placement.py` | ALL GATES PASS, exit 0 |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase9h_ui.py --require-current` | exit 1: `current-layer-commands-ui` FAILED on activation ("The owned editor never became the real key window (active=false, visible=true, windowKey=false, keyWindow=nil)"), every other probe PASS (55, including `byte-stroke-contracts-ui` whose log shows the new dab sentence); re-run ALONE (`RZ_PAINT_UI_CASES=current-layer-commands-ui`) with the same failure. The session was LOCKED (`ioreg`: `CGSSessionScreenIsLocked` Yes, HID idle ≈20 min at 00:45) — the recurring screen-lock activation case, not a code failure |
| `make cancel-bench` once, serially (load 27.94 before, 30.21 after) | exit 0: all twelve binaries ran, 19 passed, 0 failed, 0 MISS lines; the `regional_group_materialization_cancel` stall did NOT recur this run |
| `scripts/generate_mcp_reference.py` then `--check` (an owned app on port 4917, dummy key, private preference directories, terminated) | "MCP reference is current"; `mcp-tools.md` regenerated (the three catalog descriptions) |

`pgrep -f '^\./build/.*MacOS/Rasterize'` was empty afterwards and no
`scripts/**/__pycache__` was left.

## 13A-7 verification record (2026-09-25)

Receipts: `~/devel/doctored-fixtures/phase13-receipts/13a/13a-7/`
(`drive-12e2b-single-instance/` the first drive run, a MISS below, kept;
`drive-12e2b-single-instance-run2/` the passing run, each with
`receipt.json` and `work/panel-threads/sample-*.txt`; `logs/` —
`fmt.log`, `lint.log`, `make-test-1.log` / `make-test-1.uptime`,
`make-test-2.log`, `typecheck.log`, `make-app.log` / `make-app.uptime`,
`drive-*.log` / `drive-*.uptime`, `drive-9h.lockstate`, `cancel-bench.log` /
`cancel-bench.uptime`, `exits.txt`), and the regression receipts beside it
(`13a-7-regress-12h/`, `-12g/`, `-6c2/`, `-9h/`). The machine was loaded by
concurrent jobs not ours early in the run (1-minute load 133–242 around
`make test`), quieter by the drives (4.98–25.80). The latency example was NOT
run (13A-latency's, deferred).

| Check | Result |
|---|---|
| `cd core && cargo fmt --check` | clean |
| `make lint` | clean |
| `make test` run 1 (load 133.29 after) / run 2 (load 41.84 after) | 2699 passed, 0 failed, 21 ignored, both runs (a comment-only core change) |
| `make typecheck` | exit 0, 0 errors |
| `make app` (load 25.80 before, 11.33 after) | BUILD SUCCEEDED |
| `scripts/verify_phase12e2b_placement.py`, run 1, ONE instance (P11 as first written: the raw count of `_runBlocking` frame LINES) | P1–P10 PASS; **P11 FAILED** (exit 1): start 2, after P10 **3**, then 0 at every one of the ten marks over 200 edits, no soft limit. Diagnosis from the kept samples: the metric counted frame lines, not threads (the 3 was TWO threads, one showing `_runBlocking` at two offsets), and none of those threads was PARKED — each sat in the frame for only part of the one-second sample (start: 347 / 856 and 189 / 856 samples; after P10: 316 / 809 and 203 / 809), i.e. running animations of freshly opened document windows, not the 12E-2b leak (which showed each thread in the frame for the whole sample: 757 / 757). Recorded as a MISS of the metric as first written |
| the metric corrected to the quantity the criterion names — threads PARKED in `-[NSAnimation _runBlocking]` (in the frame for the whole sample) — with the EXACT equality kept, and checked before re-running against 12E-2b's own receipts: its before-fix sample at 20 edits reads 62 parked threads (63 frame lines), its after-fix end-of-200 sample 2 (2) | — |
| `scripts/verify_phase12e2b_placement.py`, run 2, ONE instance (load 7.45 before, 13.61 after) | ALL GATES PASS, exit 0: P1–P10, and P11 parked 0 at start, 0 after P10, 0 at 20 / 40 / … / 200 (frame lines 2, 2, then 0 at every mark), soft limit never named |
| `scripts/verify_phase12h_conversion.py` / `verify_phase12g_clipboard.py` | every gate PASS (16 / 13), exit 0 |
| `scripts/verify_phase13a6c2_stroke_refusals.py` (the Swift nits' paths) | every gate PASS, exit 0; the scalar reply reads "… generated from points; the scalar target stays u16.", the layer-RGB replies unchanged |
| `scripts/verify_phase9h_ui.py --require-current` (the session UNLOCKED: `CGSSessionScreenIsLocked` absent, HID idle 6 s at 01:14) | exit 0: 56 groups + pass 2, `failures` empty, `current-layer-commands-ui` PASS — the 13A-6c-2 screen-lock failure is resolved |
| `make cancel-bench` once, serially (load 5.31 before, 6.51 after) | exit 0: all twelve binaries ran, 19 passed, 0 failed, 0 MISS lines; the `regional_group_materialization_cancel` stall did not recur |

The 4× parallel rounds of the default cancel suites were not run: no
cancellation code or test changed (the one core edit is a comment). The MCP
reference was not regenerated: no tool, argument or catalog description
changed (only a reply string). `pgrep -f '^\./build/.*MacOS/Rasterize'` was
empty afterwards and no `scripts/**/__pycache__` was left.

## Measured latency

`cd core && cargo run --release --example phase13a_regional_latency` on this
machine, 2026-09-23 — the Phase 4 corpus stacked as three rasters (the corpus,
then copies at (37, 21) Multiply 0.6 and (−15, 9) Screen 0.5) under one Levels
adjustment; medians of 3; every regional output checked equal to the full
projection's crop before its time was reported.

| Canvas | Full projection (`rz_doc_flattened`) | 512×512 region | Divisor-8 preview of the whole canvas |
|---|---|---|---|
| 6000×4000 (24 MP) | 1048.1 ms | 13.1 ms | 18.5 ms |
| 8000×6000 (48 MP) | 2115.4 ms | 12.8 ms | 37.0 ms |

**Re-run 2026-09-24 (13A-5b-iii-b-3, the hot-path check).** The same command
on this machine at 1-minute load 39.8 (`latency.log`), and — to separate the
seam from the machine — the same example built from HEAD `057cbf5` (before
13A-5b-iii-b-3) in a scratch worktree and run interleaved with this build,
two rounds each, at load 37.0 → 26.0 (`latency-ab.log`). Medians of 3 (ms):

| Build | 24 MP full | 24 MP region | 24 MP preview | 48 MP full | 48 MP region | 48 MP preview |
|---|---|---|---|---|---|---|
| 13A-5b-iii-b-3 (`latency.log`) | 1096.9 | 28.9 | 41.4 | 2081.4 | 28.0 | 79.8 |
| HEAD `057cbf5`, round 1 | 1038.8 | 28.4 | 39.9 | 2207.8 | 28.4 | 80.7 |
| 13A-5b-iii-b-3, round 1 | 1065.2 | 27.9 | 39.9 | 2168.2 | 32.3 | 91.5 |
| HEAD `057cbf5`, round 2 | 1090.3 | 28.3 | 41.1 | 2130.5 | 28.2 | 80.8 |
| 13A-5b-iii-b-3, round 2 | 1095.7 | 29.0 | 40.8 | 2230.0 | 28.0 | 81.1 |

The seam is within noise of the build before it (every column overlaps
across rounds; the example passes no job control, so no seam code runs). The
region and preview columns are ≈2.2× the 2026-09-23 table on BOTH builds,
while the full projection is unchanged: that difference predates this run
(it is present at `057cbf5`); its cause — the shared machine's load or an
earlier sub-slice — was not investigated. Recorded here and as an open item,
not tuned.

**Re-run 2026-09-24 (13A-5b-iii-c-1, the open item's re-measure).** The same
command at load 28.4 (`latency.log`), then the example built from `68650dd`
(13A-1, the commit that produced the table) in a scratch worktree and run
interleaved with this build, two rounds each — first under load 39.9 → 21.7
(`latency-ab.log`), then on a QUIET machine at 4.6 → 7.3
(`latency-ab-quiet.log`). Medians of 3 (ms):

| Build (1-minute load) | 24 MP full | 24 MP region | 24 MP preview | 48 MP full | 48 MP region | 48 MP preview |
|---|---|---|---|---|---|---|
| 13A-5b-iii-c-1 (28.4, `latency.log`) | 1178.3 | 29.9 | 42.9 | 2198.7 | 28.1 | 85.5 |
| `68650dd`, loaded round 1 (39.9) | 1058.9 | 13.2 | 18.4 | 2201.8 | 12.9 | 36.5 |
| 13A-5b-iii-c-1, loaded round 1 (34.1) | 1068.9 | 28.6 | 41.3 | 2203.7 | 28.2 | 80.7 |
| `68650dd`, loaded round 2 (29.7) | 1069.5 | 12.8 | 18.6 | 2181.0 | 12.8 | 36.8 |
| 13A-5b-iii-c-1, loaded round 2 (26.0) | 1103.4 | 28.9 | 41.0 | 2155.0 | 28.1 | 80.7 |
| `68650dd`, QUIET round 1 (4.6) | 1126.5 | 13.1 | 18.6 | 2143.0 | 13.0 | 36.7 |
| 13A-5b-iii-c-1, QUIET round 1 (5.3) | 1070.3 | 28.8 | 41.2 | 2231.9 | 28.6 | 81.1 |
| `68650dd`, QUIET round 2 (6.2) | 1104.5 | 13.2 | 18.7 | 2170.2 | 12.7 | 38.0 |
| 13A-5b-iii-c-1, QUIET round 2 (5.7) | 1108.5 | 29.2 | 41.9 | 2173.5 | 27.8 | 81.4 |

The slowdown is NOT the machine: `68650dd` reproduces the 2026-09-23 table
under the same load and on a quiet machine, and this build is ≈2.2× it for
the region and ≈2.2× for the preview in every round, while the full
projection is equal. It therefore met the bisect condition (slower on a
quiet machine, both rounds agree) and was BISECTED over `68650dd..057cbf5`
(`bisect/`, each commit's example built in the worktree and run once, then
the boundary commits run a second time, `bisect/confirm/`; the load had
risen again to 44–107, which the interleaved A/B above shows the region and
preview medians do not track). Medians (ms):

| Commit | 24 MP region | 24 MP preview | 48 MP region | 48 MP preview |
|---|---|---|---|---|
| `68650dd` 13A-1 (confirm) | 12.9 | 18.4 | 12.8 | 38.4 |
| `b7dd168` 13A-2a (run / confirm) | 14.9 / 14.8 | 21.6 / 21.3 | 14.6 / 14.7 | 42.6 / 41.4 |
| `e597a6d` 13A-2b (run / confirm) | 17.1 / 16.0 | 23.6 / 23.3 | 16.6 / 15.9 | 47.8 / 45.7 |
| `c214a1b` 13A-3a (run / confirm) | 26.6 / 25.8 | 37.1 / 36.2 | 25.6 / 25.9 | 73.9 / 74.3 |
| `6c9d050` 13A-3b-i | 25.5 | 35.6 | 25.7 | 71.4 |
| `843928e` 13A-4b-ii-a | 28.4 | 40.5 | 28.1 | 79.6 |
| 13A-5b-iii-c-1 (quiet, above) | 28.8–29.2 | 41.2–41.9 | 27.8–28.6 | 81.1–81.4 |

The FIRST slower commit is `b7dd168` (13A-2a, masks / clip units / typed
Blend If: +1.8 ms region, +3 ms 24 MP preview, ≈+14 %); `e597a6d` (13A-2b)
adds ≈+1–2 ms; the LARGEST step is `c214a1b` (13A-3a, groups: +9–10 ms
region, +13 ms 24 MP / +27 ms 48 MP preview, ≈+60 %); the later commits add
≈+2–3 ms together. The fixture uses none of the constructs those sub-slices admit
(three unmasked, unclipped rasters under a Levels adjustment), so the cost
sits in shared regional machinery they changed; WHERE was not investigated
in this run. Recorded as an open item (owner 13A-5b-iii-c-2); no fix, no
tuning.

## Open items

**Standing after 13A-7** (every unstruck row below; the close-out of 13A and
P7-31 is mechanical once 13A-latency lands): **13A-latency** — 2 (the
regional latency regression; the tree plate's T(1) / T(6) against the chain);
**13B** — 16 (the regional plan in MCP `render`, the `Global` style row's
tighter input region, the typed materialization / HOLD / clip-unit / nested
`rows_k` figures, the nested level's last stage at divisor 1, the packaged
byte base's guided member, the inside-an-effect and clip-base-member and PSD
bake progress ranges, guided sampling plates, `plate_arm`'s predicate parse,
the retained parsed `Adjustment`s, the sampling plate's progress, the
regional path for the canvas display); **13D** — 6 (`MAX_WORK_UNITS` on the
typed plan, typed 24/48 MP latency, `artwork_render`'s 64M aggregate, the
brush cap numbers, the canvas-bounded selection operations, the byte
feather's missing work cap). No item is owned by 13A-7 or any other 13A run.

| Item | Owner |
|---|---|
| ~~The reports-once guards in `regional_byte_preparation_cancel.rs` are load-sensitive (first-observed value vs a report `fetch_max` overwrites ~0.1 ms later); replace them with a load-independent leak signature (progress pinned at 1.0 while the loop remains, or a value strictly between the pre-stage value and the target report, polled until the worker finishes)~~ | CLOSED by 13A-5b-i (a report log, INTERVAL + PIN; 24/24 and 12/12 under 4× parallel load) |
| ~~Every 8-bit adjustment layer carries a reveal-all mask, so it refuses regionally~~ | CLOSED by 13A-2a (masks admitted) |
| ~~Hidden clip members and baseless clipped entries are over-refused~~ | CLOSED by 13A-2a |
| ~~Layer masks, clip runs and typed Blend If~~ | CLOSED by 13A-2a |
| ~~The three 13A-1 review nits (typed disabled mask, guard order, double 11E walk)~~ | CLOSED by 13A-2a |
| ~~BYTE Blend If and fill opacity (a style with no enabled effects) still refuse "carries a layer style"~~ | CLOSED by 13A-2b |
| ~~The two 13A-2a review nits (exact typed mask plan formulas, u16 concurrent-cancel twin)~~ | CLOSED by 13A-2b |
| ~~A TYPED (16-bit / float) fill-only layer style still refuses "carries a layer style"~~ | CLOSED by 13A-3b-i |
| ~~The byte `MaskProperties::derived` feather blur runs before the loop and is UNCANCELLABLE (the reference's own preparation stage; the typed `prepare_mask` polls its job)~~ | CLOSED by 13A-5a (`derived_with_job`, checkpoints only, cache on success; measured) |
| ~~Groups~~ | CLOSED by 13A-3a |
| ~~The two 13A-2b review nits (`order` vector, drive S3 row assertions)~~ | CLOSED by 13A-3a |
| ~~Layer styles with ENABLED effects on rasters at 8 bits and every typed raster style~~ | CLOSED by 13A-3b-i |
| ~~Styled GROUPS with enabled effects at both depths ("is a group with layer-style effects")~~ | CLOSED by 13A-3b-ii |
| ~~The three 13A-3b-i review nits (split `doc_regional_tree.rs`, the 128-character module-doc line, the 158-character match arm)~~ | CLOSED by 13A-3b-ii |
| ~~The three 13A-3a review nits (structure-refusal wording, the budget open item's owner, the typed mask comment)~~ | CLOSED by 13A-3b-i |
| ~~Byte `LayerStyle::rendered` and typed `render_effects` run before the loop and are UNCANCELLABLE (the reference's own preparation stages)~~ | CLOSED by 13A-5b-i (`rendered_with_job` / `render_effects_with_job`, checkpoints only, cache on success; measured) |
| ~~`blend_interior_as_group` with a grouped interior effect on a clip base with visible members over-refuses (the reference's D6 narrowing would not route it)~~ | CLOSED by 13A-3c (admitted; D6 evaluates off there, as the reference) |
| MCP `render` does not expose the regional plan, so drives cannot read `global_analysis` / `required_input_region` | 13B |
| A layer-rect-bounded `required_input_region` for a `Global` style row (tighter than the canvas) | 13B |
| ~~Knockout and `blend_interior_as_group` with grouped interiors at both depths~~ | CLOSED by 13A-3c |
| ~~The three 13A-3b-ii review nits (zero-opacity group member charge, the `Styled` doc, the typed module-doc wrap)~~ | CLOSED by 13A-3c (nit 1 by naming it as a stated difference) |
| ~~No regional counterpart of `MAX_GROUP_BUFFER_PIXELS` / `RenderBudget` refusals~~ | CLOSED by 13A-3b-ii (materialized nests are dry-run and charged as the reference charges them; only unmaterialized levels remain a stated difference) |
| ~~Byte `rendered_group` (a materialized styled group) runs before the loop and is UNCANCELLABLE (the reference's own preparation)~~ | CLOSED by 13A-5b-ii-a (`CompositeEnv::job`, checkpoints only, per row in the chain's loops, every child mask through `active_mask_with_job`; measured) |
| ~~Inside byte `rendered_group` the style/knockout primitives are ONE unit each with no inner poll — `composite_pixels_into`, `composite_contribution`, `clamp_alpha_to_shape`, `composite_buffer_into`, `BackdropSnapshot::of`, the package strip mix, `encode_projection` / `quantize`, `Shape::of_layer`, the mask crops~~ | CLOSED by 13A-5b-ii-b (per row; measured) |
| ~~13A-5b-i's time probes in `regional_style_preparation_cancel.rs` still cancel at a calibrated TIME (load-sensitive, as 13A-5a's were); move them onto the checkpoint trigger~~ | CLOSED by 13A-5b-ii-b (`common/probe.rs`; 3/3 serial, 12/12 under 4× parallel load) |
| ~~`composite_gated_pass` panics on a zero-width accumulator (`chunks_exact_mut(0)`, carried from the 13A-5b-ii-a review)~~ | CLOSED by 13A-5b-ii-b (the lerp is skipped; pinned by `a_gated_pass_over_a_zero_width_canvas_does_not_panic`) |
| ~~The 7 typed call sites of the shared primitives (`precision_style_render.rs` ×5, `precision_style_knockout.rs` ×2) pass `JobContext::NONE`, so a typed styled composite inside the materialization is still ONE unit per primitive (typed intervals stay 13A-5b-i's); pass the live job~~ | CLOSED by 13A-5b-iii-c-2 (the live job, ONE ROW per primitive; the per-row checkpoints pinned on the worker's own log; measured PASS) |
| ~~The materialization file's (m2) freeze probe on (g′) MISSED `T_f + c_copy` 1/3 serial and 1/12 under 4× parallel load (5.8 / 5.2 ms vs ≈3 ms): `c_copy` is timed on the test thread over already-written memory; a calibration of the worker's own copy (a freshly allocated source)~~ | CLOSED by 13A-5b-iii-a (`c_copy` is the worker's own warm checkpoint timeline's gap at `issued`; 3/3 serial and parallel PASS, identification equal; the load behaviour is the row below) |
| ~~`regional_byte_preparation_cancel.rs` keeps its own copy of the probe helpers (`common/probe.rs` holds the materialization file's); fold it in when its tables are re-based~~ | CLOSED by 13A-5b-iii-a (every helper in `common/probe.rs`) |
| ~~13A-5a's factor-8 mask blur probe MISSED its bound 2/12 under 4× parallel load (the resample is calibrated on the test thread, not under the worker's load); a calibration that shares the worker's conditions~~ | CLOSED by 13A-5b-iii-a (the worker's own checkpoint timeline; identification `issued` = the longest gap's count 24/24 under load; the load MISSES persist, the row below) |
| ~~Under 4× parallel load the timeline-calibrated freeze probes still miss: factor 8 MISSED 2/12 and 3/12 (up to 144.2 ms vs 68.9 ms) with the unit correctly identified — the up resample under the probe's load runs longer than in the separately recorded timeline run; (m2) failed its verdict 3/12 in each set (the freeze, `longest / 2` of a timeline whose longest gap was a preemption, never fired inside the chain) and MISSED 1/9 in each (frozen in another unit, identification equal 1/9 and 0/9)~~ | CLOSED by 13A-5b-iii-b-1 (bench-only: `make cancel-bench`; the bench record) |
| ~~`assert_ramp` fails FALSELY under 4× load not only through its strictly-inside clause but through the scan's start: a logger that misses the whole range first sees a later stage's value above the range's end, which starts and ends the scan outside the bounds (the byte guide guard: 8 + 4 such failures and 1 + 1 strictly-inside ones in 24 runs; 0 in every serial / parallel run and 3/3 serial re-runs)~~ | CLOSED by 13A-5b-iii-b-1 (clause (2) reports a whole-range miss before the scan, naming the stall; the RAMP test is bench-only) |
| ~~Under 4× parallel load at 31–66 the untouched `regional_style_preparation_cancel::a_cancel_inside_the_plane_render_caches_no_partial_plane` failed 2/12 in one set and passed 12/12 in the other (13A-5b-iii-a); one of the two was a verdict stall AFTER its entry ("at return progress 0.0925, stage 6, 14086 checkpoints (entry 9432 + k 0)" — the freeze / calibrated `k` window), the other "returned before the entry" (the row below)~~ | CLOSED by 13A-5b-iii-b-1 for the post-entry window only (the default cache test cancels `After(0)` at the flip: no window after the entry); the pre-entry failure is the row below |
| ~~A poller stalled past the WHOLE render fails a default count-triggered cancel test in `probe`'s entry wait ("returned before the entry"; the style cache test: 1 of its 2/12 failures in one set and 0 in the other 12/12, at load 31–66 in 13A-5b-iii-a; 1/24 at load 55.9 in 13A-5b-iii-b-1, 3/3 serial re-runs pass) or at the verdict ("completed before the cancel") — the one residual of the load-robustness policy (decision above); NEW candidate: a blocking checkpoint seam on `RzJobControl` (`#[doc(hidden)]`, park at count N) that would remove it — DECIDED in 13A-5b-iii-b-2 (the plan row). In 13A-5b-iii-b-2 the style cache test failed 1/24 at load 36.0 with the stall surfacing at the VERDICT ("at return progress 0.86476, stage 6", entry 15320 — the poller's first `stage == 6` was a loop row, since `at_the_blur` no longer pins progress 0); 3/3 serial re-runs pass~~ | CLOSED by 13A-5b-iii-b-3 (the default count-triggered tests park the worker at a count from its own report log under a `Released` guard: 96/96 in two 4× parallel sets at load 29.6–68.1, no "returned before the entry" / "stalled past the stage" / "did not park" line) |
| ~~The regional 512×512 region and divisor-8 preview medians of `phase13a_regional_latency` measured ≈2.2× the 2026-09-23 table on 2026-09-24 (28–29 ms vs 12.8–13.1 ms; 40–81 ms vs 18.5–37.0 ms) at load 26–40, identically on HEAD `057cbf5` and on 13A-5b-iii-b-3, with the full projection unchanged; re-measure on an idle machine and, if it holds, bisect 13A-2a … 13A-5b-iii-b-2~~ | RE-MEASURED and BISECTED by 13A-5b-iii-c-1 (it holds on a quiet machine, load 4.6–7.3, against `68650dd` interleaved; the row below) |
| The regional render of the latency example's flat document is ≈2.2× slower than 13A-1 at the 512×512 region (≈28–29 ms vs ≈13 ms) and the divisor-8 preview (≈41 / 81 ms vs ≈18.5 / 37 ms) with the full projection unchanged — a real cost, not load (Measured latency, 13A-5b-iii-c-1): first slower commit `b7dd168` (13A-2a, ≈+1.8 ms region), largest step `c214a1b` (13A-3a, ≈+10 ms region); find the per-sample / per-render cost and remove it or state it | 13A-latency (DEFERRED to an idle machine: litecode run `phase-13a-regional-latency-regression`) |
| ~~The byte guide's progress PINS at its range's end after the first grid blur (byte and typed): `adjust_guide::build`'s two blurs share one range, the second re-ramps under `fetch_max` and holds the end through the combine; per-blur sub-ranges inside `build`~~ | CLOSED by 13A-5b-iii-c-1 (`GuideWork::blur_ranges`: each blur ramps its own sub-range, byte and typed; the two-RAMP bench guards passed) |
| ~~The byte plan's materialization scratch omits the reference's uncharged per-unit clip/style buffers inside a materialization — and, since 13A-3c, the knockout unit buffer and the base-only clone the reference allocates there too~~ | CLOSED by 13A-5c-3 (byte: `materialization_charges`' uncharged peak, pinned in `regional_resource_accounting`) |
| The typed materialization terms are sums, not peaks, and a styled raster inside a typed materialized subtree is charged twice (conservative); the byte uncharged term charges `pkg` at `S` and a pass-through's crop at `A` and adds `guide(B)` beside the later `20·A` (conservative) | 13B (tighter plan figures) |
| ~~The reference's D5 Background snapshot and an unmaterialized pass-through level's Shallow snapshot (held across later units, charged to its `RenderBudget`) are neither dry-run nor charged by the regional path, which allocates neither — a reference budget refusal caused by either does not refuse regionally (a stated difference, pinned)~~ | CLOSED by 13A-5c-3 by re-statement with cause (the budget bounds memory a nest holds; the regional path holds neither; see Decisions) |
| ~~A visible zero-opacity isolated group member of an unmaterialized unit is not dry-run at top level (a stated difference, nit 1 of 13A-3b-ii); collecting top-level zero-opacity group members for the dry run~~ | CLOSED by 13A-5c-3 by re-statement (folded into the "unmaterialized level" clause) |
| `MAX_WORK_UNITS` is not applied to the typed regional plan's `work_units` for a materialized subtree (the reference's `collect` refuses past it) | 13D |
| ~~Guided Shadows/Highlights, live filters and global operators (declare `Spatial` / `Global`)~~ | CLOSED by 13A-4a at the ROOT (guided Shadows/Highlights staged; live filters and global operators closed by recorded decision) |
| ~~A guided Shadows/Highlights inside a group, as a clip member or inside a materialized subtree still refuses "is Shadows/Highlights with a guide"~~ | CLOSED by 13A-4b-i for isolated levels and materialized subtrees |
| ~~A guided Shadows/Highlights as a CLIP MEMBER (materialized subtrees included) or directly inside a PASS-THROUGH level still refuses "is Shadows/Highlights with a guide"~~ | CLOSED by 13A-4b-ii-a for pass-through levels |
| ~~A guided Shadows/Highlights as a CLIP MEMBER (materialized subtrees and members inside pass-through groups included) still refuses "is Shadows/Highlights with a guide"~~ | CLOSED by 13A-4b-ii-b-1 for unstyled bases (materialized subtrees rendered by the reference) |
| ~~A guided Shadows/Highlights clip member of a STYLED base (styled-direct and routed member loops, the typed package) refuses "is Shadows/Highlights with a guide under a styled base"~~ | CLOSED by 13A-4b-ii-b-2 (the 8-bit package excepted) |
| A guided member of a PACKAGED byte base (`composite_package_into`'s per-strip member loop) refuses by name, since 13A-4b-ii-b-2 "is Shadows/Highlights with a guide under a packaged styled base (8-bit effects below opacity 1)" (decided in 13A-4b-ii-b-1) | 13B (tiled backing) |
| ~~`regional_guide.rs` stands at 603 lines (carried from the 13A-4b-ii-b-1 review)~~ | CLOSED by 13A-4b-ii-b-2 (the planner moved to `regional_plan.rs`; 391 lines) |
| A clip unit holding a guided member is RE-RUN from its began sample in every later fill (charged: its stages count rows from the unit's first row, conservatively the level's descendants between too); a tighter per-stage figure | 13B (tighter plan figures) |
| A nested level's LAST stage is held at divisor 1 over window ∩ buffer even for a preview (stated cost) | 13B |
| ~~The byte plan emits (and charges) the staged level of a guided isolated MEMBER group whose unit's base group has an empty extent, which the render never stages (the unit is dropped at `resolve`) — conservative figures only~~ | CLOSED by 13A-4b-ii-a (the member records are truncated under an empty base) |
| A nested level's `rows_k` count its descendants' rows too, and `input_halo` sums the halos of every stage above a row across sibling levels — conservative work / halo | 13B (tighter plan figures) |
| ~~`doc_regional.rs` / `precision_regional.rs` (the walks) stand at 758 / 681 lines, above the ~600 soft cap (they were 667 / 604 before 13A-4b-i)~~ | CLOSED by 13A-4b-ii-a (`doc_regional_walk.rs` / `precision_regional_walk.rs`; 330 / 251 lines left) |
| ~~`regional.rs` stands at 630 lines after 13A-4b-ii-a's contract paragraph (module doc), above the ~600 soft cap~~ | CLOSED by 13A-4b-ii-b-1 (the plan types moved to `regional_plan.rs`) |
| ~~A gated pass-through whose hold is missing skips its byte FINISH silently (`doc_regional_sample.rs`; the typed twin raises MISMATCH)~~ | CLOSED by 13A-4b-ii-b-1 (`debug_assert!`) |
| A HOLD is charged 16 bytes per `R_0` pixel for the whole staged render even when entered over a later, smaller region, and is never dropped after its last FINISH; a held plane is never sampled at the preview divisor | 13B (tighter plan figures) |
| ~~The byte hold fill polls every 4096 samples but reports no progress (`plane` never advances `done`), so the byte fraction under-reports by the plan's hold work until the final report~~ | CLOSED by 13A-5a (each chunk advances `run.done` and reports) |
| ~~Byte preparation PROGRESS on the UNSTAGED path: the byte plan's `work_units` count no mask blur, style planes or materialization, and the unstaged accumulator fill polls but does not report (the staged path's stage-0 `prev` / D5 `snapshot` fills report and the byte guide's blur reports inside its range since 13A-5b-iii-a)~~ | CLOSED by 13A-5b-iii-b-2 (the fill reports; mask/plane/materialization work counted, each stage inside its own range) |
| ~~Typed style planes inside a range (the typed style's `advance` stays its one report)~~ | CLOSED by 13A-5b-iii-c-1 (`prepare_style` renders under `progress.job.range(start, end)` with equal per-effect sub-ranges; the typed RAMP bench guard passed) |
| Inside ONE effect, the effect's later blurs re-ramp under `fetch_max` and hold its sub-range's end (the stated residual of 13A-5b-iii-c-1's equal per-effect split, both depths); sub-ranges inside an effect | 13B (tighter progress figures) |
| ~~`composite_styled_into`'s typed plane render (a typed materialization's styled composite) still runs under `checkpoints_only`; hand it `render_styled`'s `progress.job.range(start, end)` with a measured typed interval~~ | CLOSED by 13A-5b-iii-c-2 (`render_styled`'s `planes_job`; the ramp pinned on the worker's own log, the bench RAMP passed, the interval measured PASS) |
| A styled CLIP BASE's members render after its planes from `render_styled`'s own `completed`, so their reports sit below the planes' range end and hold under `fetch_max` until they pass it (the stated residual of 13A-5b-iii-c-2's `[completed, completed + work]` range, typed full frame and materialization alike); a member-aware split of the base's range | 13B (tighter progress figures) |
| The typed PSD bake (`bake_style16`) renders its planes under `job.range(0.2, 0.9).checkpoints_only()` — flat at 0.2, exactly as before 13A-5b-iii-c-2 (the narrowing moved to its call site); a ramp inside the bake's range | 13B (tighter progress figures) |
| ~~The measured cancel during a HOLD FILL converts to ≈21,000–43,000 "samples" on the fill's own 0.30–0.44 ns per-sample rate — above the declared 4096 — because the cancel-to-return time (0.009–0.013 ms) is the fixed return floor also seen in the stage loops (0.011–0.082 ms), not fill work; recorded as a MISS of the declared metric, not widened (see the 13A-4b-ii-a record; the u16 stage-0 ≈4109 of run 3 is the other miss; 13A-4b-ii-b-1's clip-unit hold fills: ≈27,000–91,000 at 0.011–0.034 ms; 13A-4b-ii-b-2's styled-unit hold fills: ≈19,900–38,800 at 0.008–0.012 ms)~~ | CLOSED by 13A-5a (the per-stage model `T_f + W_s · c_s`; every hold fill re-measured PASS) |
| ~~The byte guide's `blur_plane` (a staged guide's preparation) is UNCANCELLABLE, beside the mask blur and `rendered_group`~~ | CLOSED by 13A-5a (the u8 `GuideSample::blur` polls; measured) |
| ~~ICC/description preparation (`validate_context`) scratch and cancellation — P7-31's "ICC/description preparation"~~ | CLOSED by 13A-5c-1 (`context_scratch` planned and reserved; `validate_context_with_job` under the render's job, the walk polling per node, the materialization's re-validation under its live job; measured PASS) |
| ~~The `inpaint_source.rs` full-frame sampling fallback for layer metadata and masks needing effective coverage (Sample All, legacy promotion)~~ | CLOSED by 13A-5c-2a (every mask form, pointwise adjustments and opaque metadata tile; the remaining fallback runs under the sampling job, polled per row; measured) |
| ~~The `inpaint_source.rs` full-frame sampling fallback for groups and styles (Sample All over groups, raster styles; legacy promotion) — cancellable per row since 13A-5c-2a, but still full-frame~~ | CLOSED by 13A-5c-2b (the byte regional tree given a `Cut`, ≤ 1024×64 tiles, byte-identical; measured) |
| Guided (staged) sampling plates stay full-frame: a guide-reading Shadows/Highlights keeps `flattened_cut_with_job` (per-tile stages would re-run each guide over halo-grown tiles) | 13B |
| Sample-major tree plate latency against the chain: T(1) / T(6) of the tree plate (13A-5c-2b verification record) beside 13A-5c-2a's fallback 114.6 / 283.3 ms; investigate if more than 3× at equal load | 13A-latency |
| ~~The tree plate's budget and resource accounting carry the regional path's stated differences (the D5 / Shallow snapshots and zero-opacity group members not dry-run, the materialization terms, the `from_meta` transient)~~ | CLOSED by 13A-5c-3 (the tree plate reserves the admission's terms, `parse_scratch` and the uncharged nest term included) |
| `inpaint_source::plate_arm`'s predicate pass parses each contributing entry's `from_meta` under a MODELLED profile before any arm's reservation (all three arms), uncharged | 13B |
| The tree's RETAINED parsed `Adjustment`s (a LUT table per row) are covered only by the parse's ×256 factor | 13B |
| The sampling plate reports no progress (it never did): its tile loop, tree and full-frame fallback run under `checkpoints_only()`; a per-tile / per-row report | 13B |
| ~~The walk's `from_meta` transient on a MODELLED profile is uncharged (memoized above 64 KiB); charging it re-bases every byte plan pin with an adjustment row~~ | CLOSED by 13A-5c-3 (`parse_scratch` under every profile; the byte pins re-based) |
| ~~Two of the three 13A-3c review nits (the base-only backdrop helper, the re-sanitized opacity)~~ | CLOSED by 13A-4a (the helpers moved) |
| ~~The 13A-3c review nit "the two trees past ~600 lines": `doc_regional_tree.rs` / `precision_regional_tree.rs` stand at 659 / 659 lines after 13A-4a's own additions, above the ≤600 target~~ | CLOSED by 13A-4b-i (496 / 446 lines) |
| ~~The 13A-4a review nit: the 8-bit final-stage cancel estimate divides by the whole render's average~~ | CLOSED by 13A-4b-i (calibrated on its own loop) |
| Typed (16-bit / float) regional latency is not measured at 24/48 MP | 13D |
| Using the regional path for the canvas display or UI previews | 13B |
| ~~Native-precision SELECTION coverage refuses beyond the work cap even for a small selection (a 250-px feather above 1,996,008 px, morphology above 62.5 MP, Refine's summed stages)~~ | CLOSED by 13A-6a (support-bounded Feather / Smooth / Grow / Shrink / Border and Refine; exact against the retained whole-plane reference) |
| ~~ARTWORK coverage scans every edge per row (O(E·H): 58,000 edges refuse at H = 4000) and `artwork_render` caps samples + intrinsic at 64M (one canvas plane per component)~~ | CLOSED by 13A-6b (coverage: an active-edge sweep, work `n + Σ active per row`, equal to the retained full scan and never charged above it; the 64M aggregate KEPT — see the next row) |
| `artwork_render`'s 64M aggregate (one EXTENT plane per component; `artwork_render.rs:72`, `ffi_artwork.rs:119`, `NativeArtworkEvaluation.swift:69`) — a windowed component contract | 13D (targets) |
| ~~LONG STROKES (core): the brush field is a canvas-sized f64 plane (`12n`) and hard dabs integrate 8×8 everywhere~~ | CLOSED by 13A-6c-1 (a footprint-bounded field, exact interior/exterior classification with 8×8 only on the band, equal to the retained canvas-field reference and never charged above it) |
| ~~LONG STROKES (Swift): the byte walker (`SoftBrush.stampCenters`) truncates silently, `NativeBrushStroke` / the session refuse without naming the core's limits, and UNVERSIONED typed MCP point strokes rasterize promoted byte coverage (`PrecisionStrokeCoverage` ×257, `AgentServer+PrecisionPaint.swift:78-80`)~~ | CLOSED by 13A-6c-2 (one `dabLimitRefusal` sentence on every Swift path; the byte walker throws instead of truncating; unversioned 16-bit typed points generate native dabs through the one walk and rasterize through `rz_brush_u16`, refusing by name at both core limits) |
| The 50,000-dab cap (`MAX_BRUSH_DABS`) and `raw_local_coverage`'s 4,096 dabs — any cap number | 13D (targets) |
| Selection Transform (`taps²·n`), source selections (`40n`, `precision_selection.rs:78`), the typed layer-mask feather check (`precision_mask.rs:163`) and Blend (`32n`) stay canvas-bounded — by nature (a transform or source reads the whole canvas) or analysis-wide | 13D (targets) |
| The byte feather (`doc_select.rs:312`) has no work cap at all (a depth-parity gap: the typed feather is now bounded by its support, the byte one by nothing) | 13D (targets) |
