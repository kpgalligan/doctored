# Phase 9I — recommended next steps and restart handoff

This is the execution plan requested after the `2e26401` continuation and the
subsequent discussion of the remaining matting problems. The user intends to
proceed after context compaction. This documentation task does not start another
experiment or implementation phase.

**Execution update:** the [prerequisite review](phase-9i-feasibility-status.md)
records the first work package's contract, evidence inventory and conditional
pipeline choice. No candidate currently meets every prerequisite; no new model
was scored. Use that decision and its concrete dependencies as the current
handoff instead of repeating the survey below.

**Recommendation:** make the next 9I assignment a bounded feasibility milestone.
First define and evaluate assisted edge refinement, compare one substantially
different established alpha/foreground pipeline, and decide whether further
integration is justified. Keep general glass/refraction in a separate research
track. Continue independent Phase 10 work while 9I remains open.

The agreed direction authorizes preparation and evaluation of that workflow. It
does not turn an assisted result into a pass of the original automatic benchmark,
remove the existing Phase 9 completion requirements, or enable experimental
controls. Record any eventual change to those requirements as a distinct scope
decision, preserving the original failed results.

## Starting state after compaction

- Working branch: `codex-refactor`. Implementation baseline: `2e26401`.
  Read the current HEAD and working tree before starting; the documentation
  commit containing this plan will follow that implementation commit.
- 9A–9H are qualified. **9I, P7-12 and the whole Phase 9 remain open.**
  The Phase 7 ledger has 18 verified and 18 open assignments.
- Five declared candidates have failed the original combined alpha/color gate.
  V5 passes only 2 of 90 cases; those two are analytic fur cases.
- The cutout preparation foundation is verified: exact foreground/source-alpha
  retention, separate masks, captured-state publication, Undo/persistence,
  cancellation, progress-panel ordering and borrowed-input accounting.
  The last integrated checks passed 1,410 Rust tests, Clippy, typecheck, a normal
  Release build, six UI checks/15 groups and external MCP checks.
- A qualified **typed estimator and public advanced refinement workflow do not
  exist yet**. Typed output assembly is not a substitute for either.

Use this reference map for the restart. The root agent should read this plan and
the current 9I summary, then delegate detailed protocol, algorithm and evidence
reads to the appropriate owners below to conserve context. Open individual
receipts when their conclusions need checking:

1. [Current 9I status](phase-9i-cutout-status.md).
2. [V5 results and model diagnosis](phase-9i-bayesian-v5-status.md).
3. [Foreground ambiguity evidence](phase-6-foreground-evidence.md).
4. [Original metrics/input protocol](phase-6-matting-reference-protocol.md) and
   [complete-image protocol](phase-6-full-image-reference-protocol.md).
5. [Execution qualification](phase-9i-execution-status.md) and the
   [earlier ViTMatte experiment](phase-6-vitmatte-spike.md).
6. The relevant sections of the [implementation plan](professional-photo-editor-implementation-plan.md)
   and [carryover ledger](phase-7-carryovers.md).

Read root `CLAUDE.md` and the applicable `core/CLAUDE.md` or `app/CLAUDE.md` before
writing implementation code. Do not rerun completed 9G/9H work as a fresh project.

## What the failures mean

| Issue | Evidence | Consequence for the next assignment |
| --- | --- | --- |
| Unsuitable color populations | V5 estimates every pixel from boundary samples and one Gaussian per class. All 24 cropped photographs fail exact opaque-RGB preservation; inspected outputs damage hair, skin and teeth. | Correct the population/model design or evaluate a substantially different implementation. Another protection cutoff is insufficient. |
| Full-image search cost | All 24 full portrait cases exhaust V5's declared two-billion-node spatial-search budget. | Establish bounded full-size execution before committing to an algorithm or port. Increasing the cap alone does not improve its quality. |
| Ambiguous evidence | The same RGB/prior/valid constraints can describe an opaque gray mark or translucent white. The repository contains a counterexample. | Additional constraints or scene assumptions can support better estimates. Reconstruction fit alone cannot certify opacity. This does not prove the finite 90-case gate impossible. |
| Missing transmission qualification | The bottle reference supplies a silhouette, not measured transparent-interior alpha and foreground. | Acquire suitable evidence and define the supported optical model before claiming glass extraction. A numerical solution is not transmission evidence. |
| Remaining product work | Only internal assembly/preparation is qualified for native source alpha and u16 masks. | Quality success must precede typed estimator and public workflow qualification. Passing functional tests cannot waive those steps. |

The five candidates are not five independent mature matting systems. V1–V4 are
related variations; V5 is an explicitly simplified Bayesian model. Their
failures justify changing the approach, not concluding that useful matting is
unachievable. Conversely, there is no evidence that one more ordinary coding
round will finish the present 9I contract.

## Recommended product contract

Target **assisted refinement of an existing selection or mask** for ordinary
portraits, hair, fur and product edges. The user supplies intent where the image
alone is ambiguous. Keep the result reversible as foreground pixels plus a
separate editable refinement mask.

Specify these concepts independently before designing controls or a replay schema:

| Input or state | Meaning and guarantee |
| --- | --- |
| Automatic prior | An uncertain initial matte. Values 0 and 1 do not become user assertions. |
| Foreground/background constraints | Explicitly supplied categorical guidance. Preserve those labels exactly under the declared matte semantics. Record the origin of every constraint. |
| Unknown label | Releases a constraint and permits estimation. It does not certify that the covered content is translucent. |
| Alpha refinement region | Where the estimator may change the starting matte. Coverage outside it remains exact. Define any feathered boundary as actual write coverage. |
| Color correction region | Where foreground RGB may change. Keep it separate from alpha refinement and analysis scope; source RGB outside it remains exact. |
| Protected interior | Preserve captured foreground samples and the declared starting coverage exactly. Refuse contradictory protection/constraint instructions instead of silently choosing one. |
| Analysis region | May provide contextual colors beyond the write regions. Reading a pixel does not authorize changing it. |

Protection and local writeback are enforceable editing guarantees. They do not
establish the physical truth of the starting mask or the opacity of an unknown
pixel inside a correction region. Keep the original reference-opaque metric:
damage to an opaque pixel inside an authorized region is still a quality failure.

For typed sources, source alpha and refinement coverage remain separate. A
foreground constraint on the refinement matte must not force the stored source
alpha to 65535 or erase hidden RGB. Define the estimator's interpretation of
partially transparent source pixels explicitly before implementing that path.

Before/After should show the captured original and the exact candidate over
useful replacement backgrounds, with native-pixel inspection. Apply must publish
the reviewed candidate without recomputing it. Uncertain or unsupported cases
must preserve the original on refusal and let the user revise guidance.

These are target semantics for research and eventual implementation. Keep current
advanced controls disabled until the applicable quality and integration gates pass.

## Two separate evaluation protocols

### Preserve the original automatic benchmark

Retain all 24 analytic, 24 photographic crop and 42 complete-image cases, their
original dimensions, profiles, backgrounds, degraded priors and reference pins.
The established recipe that derives the initial degraded prior remains unchanged;
no additional truth-derived input may be introduced.

Keep every original per-case requirement:

- Boundary-alpha MAD may regress by at most `1/255`; soft-detail recall is at
  least 95%.
- Reference-opaque source RGB remains exact.
- Alpha-weighted foreground MAD may regress by at most `1/255`.
- Fractional foreground MAD is at most 0.05 and improves by at least 20% when
  its initial value exceeds 0.02.
- Recomposition boundary MAD is at most 0.02 on each of white, dark and checker.
- Refusals fail. Missing fractional reference coverage cannot provide a color pass.

Preserve native dimensions and the existing 2 GiB harness policy. Declare the
algorithm's own aggregate work bounds and approximation semantics separately.
Do not substitute resized images, score only successful cases, select a different
winning algorithm per case, or relabel diagnostic fallback images as outputs.

Compare against the preserved V3 result and V4 unchanged-alpha control, alongside
the original sampled/closed-form records. V5 remains a useful diagnostic branch,
not the strongest overall baseline. Select comparison baselines globally before
scoring. Old reference-alpha color rows belong to the old foreground estimator;
they are not evidence that a new alpha model or joint pipeline will pass.

### Add a separately identified assisted protocol

Assisted inputs change the available evidence. Give this protocol its own version,
declaration, reports and completion claim. Reuse the original metric definitions
where the reference model applies, but keep results separate from automatic scores.
Before annotation or scoring, freeze the exact case manifest, subject categories,
native dimensions, reference eligibility, interaction/time budget and whole-run
pass rule. Require every declared quality case to meet its applicable original
quality thresholds, protected-region guarantees and resource requirements, with
the evidence for each claimed category present. Missing qualifying evidence is
not a pass; do not remove difficult cases or narrow categories after seeing results.

1. Export only observed source RGB, the permitted starting prior, dimensions and
   profile information into an annotation/inference directory. Keep reference
   alpha/F, scoring files and failure maps out of that directory.
2. Define the annotation rules and a bounded interaction budget before collecting
   guidance: allowed labels, stroke sizes/counts, protection and correction regions,
   conflict handling and when an annotator may inspect a preview.
3. Collect genuinely source-only guidance. Human annotations must be made without
   seeing reference planes or score feedback. AI-suggested labels must be labeled
   as model output and cannot masquerade as human constraints or verified opacity.
   They stay uncertain unless a real user explicitly accepts them as guidance;
   record both their origin and that acceptance. Acceptance establishes user
   intent, not ground-truth correctness.
4. Freeze replay, input hashes and provenance before scoring. Use the same recorded
   guidance for every compared candidate. A changed protocol becomes a new declared
   experiment, with the earlier run intact. A new identity never permits
   annotations derived from scores, failure maps or reference planes: recollected
   guidance still needs source-only blinded provenance. For the first comparison,
   use source/prior inspection; any later interactive preview-feedback protocol
   must declare its preview source and allowed rounds separately, without truth feedback.
5. Score all selected cases, including guidance mistakes and refusals, and separately
   measure exact preservation outside write regions and inside protected regions.
   A footprint cannot exempt an opaque-color error from the reference metric.

Use a fresh annotation context that has not seen the ground truth. If actual human
guidance is required, prepare a concrete source-only annotation task. Continue
independent baseline/evidence work while waiting; do not invent user strokes.

An assisted pass establishes only the declared assisted workflow. It cannot
silently satisfy the existing automatic completion gate. Preserve that distinction
in status documents and the carryover ledger.

## Work packages and order

### 1. Contract, baseline selection and evidence audit

Produce a short decision record before the next scored candidate:

- Finalize the assisted semantics above, output meanings, constraint conflicts,
  precision limits and the distinction between protected pixels and inferred opacity.
- Review at most three plausible established implementations. Select **one**
  substantially different complete alpha-and-foreground pipeline for the first
  comparison. If a model returns only alpha, name and qualify the foreground
  reconstruction stage too.
- Check author preprocessing, expected inputs, profiles/domains, code/weight rights,
  source sizes and a credible full-size memory/work forecast. Prefer a faithful
  reference evaluation before attempting a Rust or Core ML port.
- Read the preserved ViTMatte results first. Repeating the same checkpoint, unsafe
  automatic hard labels or previously failed full-size memory policy is not a new
  experiment. A change of preprocessing, tiling, backend or input policy needs a
  separately declared hypothesis and its own evidence.
- Inventory existing fixtures and tools. Confirm file hashes and availability;
  temporary environments and artifacts may have disappeared after compaction.
  Recover missing fixtures through the existing pinned acquisition procedures.
- Record what photographic transmission/foreground evidence is available and what
  remains unavailable. Resolve intended use and dataset/weight permissions separately.

Deliverable: one selected pipeline, a frozen input protocol, an acquisition/runtime
manifest and a feasibility declaration. If no candidate meets these prerequisites,
record that decision and the concrete missing evidence; do not substitute another
unexamined custom variant just to keep producing code.

### 2. One bounded feasibility comparison

Implement only the adapter and independent contract checks necessary to evaluate
the selected pipeline. Keep runtime dependencies, weights and image artifacts
outside the application and repository. No public control or production model
runtime is added at this stage.

Before scoring, freeze the candidate identity, source/weight/package hashes,
preprocessing, RGB domain, constraint policy, alpha/F quantization, resource policy
and success/refusal semantics. Models receive only their declared inference inputs.
Check output dimensions, finite values, range, channels and constraint preservation
before any conversion can conceal malformed output.

Run the complete original 90-case comparison with every refusal retained. If an
assisted protocol is ready, evaluate it separately with the same frozen guidance
across baselines. Record:

- Alpha, foreground and combined results independently, with per-case reasons.
- Counts for admitted outputs separately from unchanged-prior/refusal diagnostics.
- Original pixel dimensions and all resource refusals; model/candidate time
  separately from import, scoring and artifact generation.
- Peak planning estimates and applicable measured memory, including model state,
  preprocessing, inference, F reconstruction, scoring and retained output overlap.
  A sampled memory monitor is not an operating-system allocation ceiling.
- Native-pixel comparisons of contours and opaque interiors, including hair,
  skin, eyes/teeth, fur texture, product detail and replacement backgrounds.
- Independent gate recomputation for every newly scored run. Additionally require
  legacy parity whenever shared scoring code changes.

Do not run multiple large model jobs concurrently without a combined memory
forecast. The earlier oversized UI test was refused for memory risk; the verified
4096² tests succeeded after correcting progress-panel scheduling. That issue is
resolved and is not a reason to enlarge fixtures or alter resource policy again.

### 3. Make a recorded decision, then stop this experiment

| Result | Required next action |
| --- | --- |
| Original quality, full-size execution and required evidence all qualify | Proceed to typed estimator qualification, then public workflow integration. Do not mark 9I complete yet. |
| Assisted evaluation qualifies but the automatic gate fails | Record a distinct assisted capability and its evidence. Keep original results and 9I/P7-12 open under the current plan. Any changed completion scope must be explicit. |
| Clear improvement but some original cases still fail | Record the exact failures and a proposed next hypothesis. End this milestone without enabling controls or automatically launching a sequence of tuned variants. |
| Same interior damage, preservation/decontamination tradeoff or resource refusal | Stop that algorithm line. Revisit evidence, the intended workflow, or defer the research while independent product work proceeds. |
| Missing inputs, rights or reference evidence | Record the precise dependency and preserve other progress. Do not manufacture a background capture, substitute silhouette truth or infer permission from a software license. |

**Recorded outcome (2026-09-15).** Rows 4 and 5 both applied and the owner has
now made the scope decision they required. The automatic algorithm line is
stopped: five candidates, the same interior damage and preservation/decontamination
tradeoff, and no eligible reference pipeline. RVM is dropped (its weight-license
issue is unanswered, its GPL-3.0 code cannot link into an all-rights-reserved
Rasterize, and it is trimap-free human-video work that cannot use brush guidance),
so no neural matting model is added and Apple Vision stays the automatic prior.
The gate is rescoped once as [gate v2](phase-9i-gate-v2-status.md): thresholds
unchanged, the analytic pass rule reduced to the twelve recoverable cases,
ambiguous cases reported as diagnostics, glass moved to the transmission-research
track below, and the exact-opaque-RGB clause replaced by a bounded rule. Under row
2's "any changed completion scope must be explicit" clause, completion is now the
assisted protocol passing gate v2 on a declared 36-case subset; the automatic path
is reported only. All five preserved candidates are re-scored under both gates in
that document.

Numerical implementation defects may be corrected and independently tested. If
they affect a scored pipeline, retain the old run and give the corrected run a new
identity. Do not present post-score changes as part of the original frozen result.

If a custom successor to V5 is chosen in a later milestone, its hypothesis must
address spatially representative interior samples, multiple foreground/background
color populations, uncertainty and bounded full-size queries. A higher alpha
cutoff, lower residual or larger search cap does not establish those properties.

## Separate glass and transmission research

Define whether the intended deliverable is silhouette selection, ordinary scalar
alpha/transmission, or refractive recomposition. Those are different claims.
An RGBA foreground plus alpha cannot generally reproduce how refractive glass
warps a different background. [TOM-Net](https://guanyingc.github.io/TOM-Net/)
uses an object mask, attenuation and refractive flow for that richer problem.

For ordinary transmission qualification, obtain appropriately licensed photographic
alpha and foreground evidence with documented capture/annotation semantics and
color domain. Preserve the limitations of measured/noisy references; do not
silently import linear high-depth data into the existing encoded-RGB exact gate.
The current bottle remains silhouette-only evidence.

An independently captured empty background can provide stronger observations in
a separate workflow, as required by [Background Matting V2](https://github.com/PeterL1n/BackgroundMattingV2).
It must be an actual independent capture, not a reconstruction from benchmark
reference alpha or foreground. This is an option to evaluate, not a replacement
input silently granted to the existing automatic benchmark.

Record missing rights or data concretely. This plan does not authorize contacting
publishers, submitting agreements or making purchases. Such a dependency does not
prevent contract work, eligible local comparisons or Phase 10 work.

## Typed and public integration after feasibility

Once an applicable quality/evidence gate qualifies, assign these as separate
reviewable implementation slices:

1. **Typed estimation:** retain genuine source RGB/alpha, profile semantics and
   hidden colors; separate source alpha from u16 refinement coverage. Disclose
   model analysis precision. Never substitute byte preview pixels for a native
   master or imply that promoted byte output contains newly recovered u16 detail.
   Use independent tests for tiny/partial alpha, native coverage, color domains
   and exact protected/outside-region preservation.
2. **Captured refinement workspace:** original-based previews, clear guidance and
   correction/protection regions, native-pixel Before/After and exact candidate
   Apply/Cancel. Retain document/selection/target/session identity across work.
3. **UI/MCP/Actions parity:** versioned replay of actual inputs, settings and
   guidance; strict parsing; bounded payloads and one central Undo transition.
   Refuse stale, closed, cancelled, conflicting or unsupported requests cleanly.
4. **Reusable masks and persistence:** independent mask transforms must preserve
   foreground/source-alpha samples; qualify channel reuse, native reopen,
   snapshots and recovery, including the supported public workflow.
5. **Final execution and compatibility:** reuse the qualified compound controls,
   progress factory, leases and admission paths. Check every new estimator stage,
   cancellation boundary and simultaneous retained allocation. Verify actual
   Release UI/MCP behavior and unchanged legacy meaning before exposure.

Use the existing pure Rust model and FFI/header/`RasterCore.swift` contract. Put
new Swift feature logic in dedicated files and preserve central history. Run the
checks appropriate to each change; full Rust/Clippy and typecheck/Release/actual
app checks remain required for final implementation qualification. Documentation
alone does not require rebuilding the application.

## Independent roadmap progress

Continue Phase 10 work without waiting for matting research. The implementation plan
already permits dependency-based ordering.

- **10A is qualified and closed** within the [bounded import contract](phase-10a-qualification.md).
  Independent GIMP source/layered/merged-reader evidence, exact storage and
  metadata, compatibility reporting, actual UI/MCP and full regression checks
  pass. Its excluded mask forms, profiles, compression and advanced constructs
  do not keep this completed subset open. Any future import extension needs a
  separate explicit assignment; export and high-depth/advanced PSD belong to
  10B/10C.
- **10D:** [basic blending](phase-10d-basic-blending.md),
  [seven additional separable modes](phase-10d-separable-status.md),
  [four further modes](phase-10d-four-modes-status.md) and
  [five additional native modes](phase-10d-five-modes-status.md) have separate
  qualification records. Continue the remaining native blend/style semantic
  groups, coordinating shared core/FFI files through one integration owner.
- **10B/10C:** claim exchange support only for constructs whose internal semantics
  and independent-application reopen checks qualify. Native-depth exchange also
  depends on the existing Phase 7 foundations.

Assign each next 10D semantic group or separately chosen 10B/10C exchange slice
when a subagent slot becomes free. Do not reschedule the completed 10A contract
as an unbounded import task.
Starting or finishing Phase 10 does not close 9I or Phase 9.

## Subagent assignments and commit boundaries

Use a root integrator and at most three active subagents initially:

| Owner | First bounded assignment | Output |
| --- | --- | --- |
| Root | Maintain evidence/status, resolve shared-file ownership, review and integrate | Current decision record and explicit commits |
| Contract agent | Specify assisted semantics, conflicts, provenance and evaluation protocol | Reviewable contract and replay/annotation specification |
| Baseline agent | Assess up to three reference pipelines and select one eligible complete pipeline | Pinned candidate choice, adapter/resource plan and declaration |
| Evidence/review agent | Audit references/rights, input isolation, metric independence and plans | Concrete findings and evidence inventory |

After selection, the baseline agent owns the one declared comparison and the
review agent independently checks inputs, outputs, resource refusals and metrics.
As a slot frees, schedule the next bounded 10D assignment without conflicting ownership.
Use isolated worktrees for concurrent implementations. Do not apply stale patches
from the earlier 9F-based research worktree over the current branch.

Commit the contract/decision documentation, each complete declared experiment,
and each verified implementation slice separately. A failed research comparison
may be committed as research with its failures explicit. Only mark a phase or
carryover verified when its applicable completion requirements actually pass.
Stage owned paths explicitly and preserve unrelated user work.

## Resume instructions

The next action after compaction is work package 1, with the subagent assignments
above. Do not interpret a new “continue” as permission to tune V5 repeatedly,
enable the advanced controls, or restart all completed Phase 9 slices.

1. Read this plan and the current 9I status; check HEAD/working tree and relevant
   per-side rules. Treat this conversation's accepted direction as continuing
   authorization for the planned work; do not ask for the same general approval.
2. Verify that the existing reference manifests and runtime artifacts still exist.
   Key preserved evidence is under
   `~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-joint-bayesian-v5/` and
   `~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-v5-diagnostics/`;
   `/private/tmp/rz-phase9i-continue-parity/` was purged from /private/tmp and is
   not preserved. The corpus is under
   `~/devel/rasterize-fixtures/phase6/{references,full-references,photos}`. Exact
   checksums and reconstruction commands belong to the linked status/protocol
   documents.
3. Preserve the qualified implementation baseline and existing corpus. Reuse the
   current selection/mask and execution infrastructure instead of rebuilding it.
4. Produce the contract, eligible baseline choice and evidence inventory before
   starting one frozen comparison. Keep the automatic and assisted protocols
   distinct and choose a recorded next action from the decision table.
5. Continue independent Phase 10 slices as capacity permits, committing their
   verified work separately and retaining the open 9I status.

The expected outcome of the next 9I round is a supported feasibility decision.
Another round may establish a viable approach; it is not promised to complete the
present 9I scope, and typed/public qualification will still be required afterward.
