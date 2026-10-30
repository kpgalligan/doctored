# Phase 9I assisted edge refinement protocol v1

Protocol identity: `phase9i-assisted-edges-v1`. This defines a separate, bounded
source-only human-guidance experiment requested by the
[next-steps plan](phase-9i-next-steps.md). It is an input/output contract, **not a
frozen scored run or a qualified capability**. The accompanying
[declaration template](../designs/phase-9i-assisted-v1.template.json) enumerates the exact
case population; its missing input, implementation and replay hashes must be
filled and frozen in a new private run before scoring. No human annotation has
been collected for this protocol.

The original automatic 90-case protocol, failed results, 2 GiB policy and Phase
9I/P7-12 completion requirements remain intact. An assisted pass cannot close
them. Advanced controls remain disabled. The byte experiment below does not
qualify a typed estimator or the eventual public workflow.

**Gate v2 amendment (2026-09-15).** This protocol keeps its 78-case ordinary-edge
population and its 78/78 whole-run report exactly as specified below; nothing in
the amendment reduces what a run must report. Three pointers are added.
(1) *Completion subset.* 9I completion is decided on the 36-case subset declared
in [gate v2](phase-9i-gate-v2-status.md) — all 24 photographic crops plus
`full-<image>-checker-{binary,blurred}` for each of the six non-bottle full
images. The bottle stays excluded, as this protocol already declared.
(2) *Opaque preservation.* The "remains exactly equal" bullet below is replaced,
for gate v2 scoring, by gate v2's bounded opaque rule: at most 2 code levels, at
most 1% of reference-opaque pixels, and every changed pixel inside the Rust
`boundary` set. (3) *Annotator clause.* The owner is the annotator; before the first
annotation the owner lists in the run declaration every case id they have already inspected
(source, reference, comparison panel or scores). Those are annotated unblinded and
flagged `annotator_exposed: true`, and all 36 subset cases stay in the
denominator.

## Fixed population and claim

The quality population is exactly **78 cases** selected for ordinary portrait,
hair, fur and hard-product edge refinement. The template explicitly lists each
original case ID and native dimensions, in lexicographic case-ID order.

| Family | Subjects | Backgrounds and starts | Cases / dimensions |
| --- | --- | --- | --- |
| Analytic | `portrait_hair`, `fur`, `hard_product` | `light`, `dark`, `ambiguous`; `blurred`, `binary` | 18 at 320 × 256 |
| Portrait crops | PhotoMatte `0003`, `0031`, `0060`, `0088` | `light`, `dark`, `checker`; `blurred`, `binary` | 24 at 1280 × 1280, original `(512,384)` crop |
| Full fur | `fur-longhair` | Same photographic backgrounds/starts | 6 at 763 × 764 |
| Full hard product | `hard-arduboy` | Same photographic backgrounds/starts | 6 at 944 × 1338 |
| Full portraits | Same four PhotoMatte references | Same photographic backgrounds/starts | 24 at 2304 × 3456 |

The six `translucent_glass-*` analytic cases and six `full-glass-bottle-*` cases
are excluded from this **new ordinary-edge claim before annotation or assisted
scoring** because their optical subject is outside its scope. The existing
reference protocols already identify the bottle as a silhouette with no
transparent-interior foreground/alpha truth. This is an eligibility decision,
not a response to assisted scores; all twelve remain in the automatic 90.
Changing this population requires a new protocol version and retains v1.

Use the original pinned references and unchanged source-compositing and
degraded-prior recipes in the
[crop/analytic protocol](phase-6-matting-reference-protocol.md) and
[complete-image protocol](phase-6-full-image-reference-protocol.md). The legacy
prior's derivation from the reference is the existing permitted construction;
it authorizes no additional reference-derived guidance. Native dimensions,
backgrounds, profiles and starting masks cannot be substituted.

All 78 cases require nonempty fractional reference coverage and the original
alpha and foreground metrics. Their supported evidence remains supplied
annotations: analytic construction, green-screen portrait extraction, edited
fur and hard-product cutouts. An assisted pass supports this annotated
ordinary-edge corpus; it establishes neither measured physical opacity nor
photographic transmission or refractive recomposition. Evidence missing for a
claimed category blocks that category; it is never counted as a pass.

## Input, output and precision contract

`I` is captured straight RGB8 in the benchmark's encoded-RGB domain, `S` the
captured source-alpha plane, and `M0` the captured byte refinement matte. Every
v1 input has `S=255` everywhere. Dimensions, row-zero-at-top orientation and
profile identity are fixed. No linear-light substitution, rescaling, automatic
profile conversion or initial matte replacement is allowed. A candidate must
declare any model-internal normalization, padding, tiling and reconstruction
without changing the admitted image extent.

An admitted result is `(F, S, M)`: straight foreground RGB8, **unchanged captured
source alpha**, and a separate editable refinement matte M8. `F` estimates an
ordinary scalar-alpha foreground; it is not premultiplied RGB, an empty-background
capture, or a refractive-flow field. Effective displayed coverage is
`(S/255) × (M/255)`. In v1 it equals `M/255`. Source samples and the original
matte remain retained for reversal; returning a flattened alpha product is not
the declared output.

The general editing meaning of an FG constraint is `M=full`, not `S=full`:
for a native source with `S16=32771`, FG would retain `S16=32771`, every hidden RGB
sample and a separate `M16=65535`. This example specifies storage intent only.
**Partially transparent sources, native RGBA16/HDR/linear/profile-converted
inference and u16 output estimation are unsupported by v1 and must refuse.**
They need a separately declared source observation model and typed estimator
qualification; byte results cannot demonstrate retained u16 distinctions.

Before quantization or writeback, inspect the candidate's raw native-size
alpha and straight foreground RGB arrays for exact extent/channel count,
finite values and range `[0,1]`. Nonfinite, out-of-range, missing or malformed
output refuses; clipping or replacing it before this check is forbidden.
Record raw validation and any declared constraint-enforcement stage separately.
After valid output and the write equations below, quantize once with
`Q8(x)=floor(255*x+0.5)`. Directly copy captured bytes on protected or zero-write
paths so floating-point round trips cannot weaken exactness. Score the final
stored bytes, including quantization error. A numerical confidence field is
diagnostic only and never authorizes writes or certifies opacity.

## Independent guidance and write regions

All planes cover the entire captured canvas. Their meaning does not depend on
which algorithm is compared.

| Plane | Encoding and meaning |
| --- | --- |
| Prior `M0` | u8 uncertain coverage, including codes 0 and 255; never an explicit user assertion. |
| Labels `L` | u8 categorical `0=Unknown`, `1=FG`, `2=BG`. FG requires `M=255`; BG requires `M=0`. Unknown releases an earlier label and permits estimation; it does not assert translucency. |
| Alpha-write `A` | u16 actual coverage for changing `M0`, with 0 exact no-write and 65535 full-write. |
| RGB-write `C` | Separate u16 actual coverage for changing `I`; it does not imply permission to alter alpha. |
| Protection `P` | Binary u8 0/1. At 1, preserve captured `I`, `S`, and `M0` exactly; protection makes no opacity claim. |
| Analysis `R` | Full canvas in v1. Reading contextual colors never expands `A` or `C`. Cropped analysis needs a new declared policy. |

Initial state is Unknown everywhere, `A=C=65535`, `P=0`, full-canvas analysis.
The annotator may independently restrict write regions and protect interiors.
No alpha cutoff or automatic trimap becomes a human label. An implementation
may derive uncertain model inputs from the prior under its frozen policy, but
must disclose those separately from `L`; it cannot overwrite user provenance.

Let `a=A/65535`, `c=C/65535`, and set both weights to zero where `P=1`. Let
`m_hat`, `f_hat` be valid normalized candidate estimates. Where `L` is FG/BG,
replace `m_hat` by the corresponding exact endpoint in the declared output
constraint stage. The final result is:

```
M = Q8((1-a) * (M0/255) + a * m_hat)
F = Q8((1-c) * (I/255)  + c * f_hat)   # independently for R, G and B
S = captured S                        # exact byte copy everywhere
```

Feathered regions are actual fractional write coverage in these equations,
including every nonzero feather pixel. A feather is not a display-only outline
and cannot be thresholded to full permission. `C=0` preserves RGB even if alpha
changes; `A=0` preserves the matte even if RGB changes. Protection overrides
both write weights by explicit contract, while contradictions below refuse.
Hidden RGB at output matte zero remains subject to the same RGB-write rules.

Conflict validation precedes estimation and must not silently choose a winner:

- Simultaneous FG and BG assignments in imported planes, invalid label/mask
  values, wrong extents, missing provenance or unsupported precision refuse.
- A protected pixel with FG/BG whose `M0` is not the requested endpoint refuses.
- An FG/BG pixel with `A<65535` whose `M0` is not the requested endpoint refuses:
  exact categorical intent cannot override a zero or feathered write boundary.
- A requested endpoint already present in `M0` is compatible with protection or
  partial/zero alpha-write coverage. Labels never require any RGB change.
- A later valid logged label gesture replaces that pixel's earlier label; an
  Unknown gesture releases it. Protection is changed by its own explicit
  gesture. Reject the entire conflicting gesture, retain preceding state and
  report the conflict; never repair a conflict by enlarging a write region.

Refusal, cancellation, stale capture or failed validation retains the original
without publication. An unchanged-prior diagnostic is labeled as such and is
not an admitted candidate output. Eventual Before/After must show the captured
original and exact candidate over white/dark/checker with native-pixel
inspection; Apply must publish that reviewed candidate without recomputation.
This future integration requirement does not grant v1 annotators preview access.

## Source-only annotation task and bounded interaction

Use one fresh human annotator for each case, with one attempt per case. The
person must not have seen reference planes, published cutout originals,
candidate outputs, score reports or error maps for these fixtures. No agent
that has read the reference/score context may generate substitute strokes.
Do not send this scorer-facing document or its case-to-reference mapping into
the annotation workspace; use the task card below and anonymous source files.

Before annotation, prepare a new private directory containing **only** anonymous
case IDs, observed source RGB, permitted starting prior, dimensions and profile
information, plus the task card and empty replay writer. PNG sources must be
RGB with no reference alpha, hidden metadata, embedded preview or filename that
points to truth. The exporting/scoring process may access pinned references to
produce the original permitted inputs; the annotation and inference processes
must not have access to that tree. A path allowlist alone is not isolation if
the process can read the reference directory. Preserve attribution evidence in
the private evaluator's artifact tree; this protocol authorizes no redistribution.

Freeze all 78 source/prior file and decoded-plane hashes, profile identities,
anonymous mapping, export program hash, annotation program/rasterizer hash and
this protocol hash **before the first annotation**. The template's null hashes
are readiness blockers, not wildcards. Record accessible files and annotator
blinding attestation. Missing inputs or exposure invalidate readiness; they are
not permission to infer guidance from references.

The exact v1 interaction budget is **300 elapsed seconds and 20 modifying
gestures per case**, whichever is reached first. Start the timer at first
source/prior display. Zoom/pan and toggling source/prior/guidance overlays do
not consume gestures but do consume time. No pauses or budget transfer between
cases; preload/decode precedes the timer. Across 78 cases the maximum annotation
time is 23,400 seconds (6 hours 30 minutes). Keep incomplete/zero-gesture cases.

Allowed tools are FG, BG, Unknown, protect, unprotect, and independent replace
alpha-write / replace RGB-write region. Each pointer-down/up stroke, polygon
commit, Undo, Redo and rejected modifying gesture consumes one gesture. Replacing
a region with full or empty canvas also consumes one. No text prompt, imported
mask, auto-selection, numeric alpha painting, model suggestion or copying a
previous case's guidance is allowed. Brush diameters are exactly 3, 9, 27 or 81
native pixels; each stroke records at most 256 integer pixel-coordinate points.
Region polygons have at most 32 integer-coordinate vertices. Region feather
width is one of 0, 4, 16 or 64 native pixels; write planes retain u16 values.
Labels/protection use hard binary footprints. Geometry is fixed as follows:

- A stroke point `(x,y)` denotes native pixel center `(x+0.5,y+0.5)`, with integer
  `0<=x<width`, `0<=y<height`. Its footprint contains pixel centers at Euclidean
  distance at most `diameter/2` from the polyline, including round end caps; a
  single point makes a disk. Clip to the canvas; no pressure or antialiasing.
- A region is one simple closed polygon with 3 to 32 vertices in native
  pixel-boundary coordinates, integers in `[0,width] × [0,height]`. Reject
  self-intersections, repeated vertices, zero-length edges and zero-area shapes.
  Test pixel centers with even-odd fill; centers exactly on an edge are inside.
  Replacing a region replaces its entire plane; outside is zero.
- At feather width zero, inside coverage is 65535. At positive width `w`,
  inside coverage is `floor(65535*min(1,d/w)+0.5)`, where `d` is the shortest
  Euclidean distance from that pixel center to any polygon edge. Outside is
  zero, including no outward spread; the exact edge has zero feather coverage.
  Full/empty-canvas commands directly set all codes to 65535/0 without feather.

Freeze and verify the annotation program's implementation of these conventions
before collecting guidance; the resulting planes plus complete events must
replay to their recorded hashes. Tool/rasterizer implementations do not yet exist
for this protocol, so neither collection readiness nor numeric verification is
claimed here.

There are **zero candidate-preview rounds**. The annotator sees source/prior
and their own edit overlays only, with no replacement-background candidate,
reference or score feedback, including across later cases. Submit locks that
case permanently. After budget expiration, retain the last valid state and mark
`budget_exhausted`; annotator abandonment is retained as `incomplete`, not dropped.
Only input-contract errors, never quality hints, may be shown during annotation.

Every event records protocol/session/case IDs, actual human origin, tool, native
geometry, radius/feather, monotonic elapsed time, gesture index, result and plane
hashes. Each session records a pseudonymous human ID and actual start/end time,
source-only attestation and displayed asset hashes. Freeze replay and final
`L/A/C/P` hashes before any candidate is scored. Use the exact same guidance
across globally selected comparisons; candidate-specific human correction is
outside v1. Do not fill absent human provenance with an assistant's identity.

AI-suggested labels are outside v1. A later version must retain model identity,
source-only input hashes, original uncertain suggestions and a real user's
explicit acceptance event. User acceptance establishes editing intent, not
reference correctness. A later preview-feedback protocol must separately declare
its fixed preview source and rounds; neither version may use truth/score feedback.

**Task card for the fresh annotator** (show the standalone
[annotation task](phase-9i-assisted-annotation-task-v1.md) with anonymous
source/prior only):

> Refine the intended subject's edge using the observed photograph and starting
> mask. Mark confidently intended foreground/background; release a mark with
> Unknown when uncertain. Protect any area whose current pixels and mask must
> stay exact. Restrict alpha and color changes independently where useful. A
> foreground mark means fully included in the refinement mask; it does not
> prove physical opacity. You have 5 minutes and 20 editing gestures for this
> case. You may inspect source, prior and your guidance at native pixels. Do
> not consult external images, reference masks, model outputs or scores. Submit
> once; unmarked pixels and annotation mistakes remain in the evaluation.

No annotation program, source-only export or genuine human session is claimed
by this document. Those concrete assets and their hashes are dependencies for
collection. Independent automatic pipeline/evidence work can continue meanwhile.

## Tooling pins (2026-09-15)

The collection tool and its replay format are frozen here. Nothing in this
section changes the population, the gates or the pass rule above; it records
what a run's `program_sha256` must pin and what a replay means.

- **Replay format.** `core/src/assisted_replay.rs` — protocol
  `phase9i-assisted-edges-v1`, replay schema 1. Its module doc is the schema
  table, and `rz_assisted_replay_plane` / `rz_assisted_replay_summary` are the
  only readers. The core validates a whole document, in progress or final; it
  never hashes, decodes a PNG or scores.
- **Rasterizer.** The label plane is an all-128 base with the reconstructed
  ACTIVE strokes applied in order through the unchanged `rz_trimap_strokes`
  constructor, `radius = diameter / 2`, points at pixel centers
  `[x + 0.5, y + 0.5]`. Its 64-million segment-box visit cap therefore applies
  to an assisted session exactly as it does to any other stroke payload.
- **Session semantics.** The timer starts in `begin()`, which the task card's
  Begin (human) or `assisted_open_case` (synthetic) calls; decoding and the
  four displayed-asset hash checks precede it, and the source reaches the
  canvas only after it returns. A stroke's recorded samples are integer canvas
  pixels with consecutive duplicates removed and clamped to the canvas; more
  than 256 samples are decimated to 256 keeping the first and last and evenly
  spacing the rest. **The recorded points are what is rasterized and shown.**
- **Gesture accounting.** A stroke past the 5-minute budget is recorded
  `rejected:time_budget`; a stroke the rasterizer refuses is recorded
  `rejected:rasterizer_limit`. Both cost a gesture and leave the plane and its
  hash unchanged. Undo needs a non-empty active stack and Redo a non-empty redo
  stack; an accepted stroke clears the redo stack; the 20th modifying gesture
  ends the case as `budget_exhausted`. A one-second timer is the fallback that
  ends an over-time session, closing an in-progress stroke first.
- **Labels-only collection.** This tool offers the three label brushes and
  nothing else — no protect tool, no region tools, no polygons and no feather.
  A replay therefore declares constant write planes (`A = C = 65535`,
  `P = 0`), and `write_planes` records the sha256 of those constant u16
  little-endian planes rather than collected geometry. The protocol's
  independent-region clauses stay as written and are simply unexercised by v1
  collection.
- **Provenance.** `human_origin == (origin == "human")` and
  `scoreable == human_origin`. A human session requires a non-empty
  `annotator_id` and a `source_only` attestation and refuses every agent stroke
  and agent finalize; a synthetic session records `annotator_id: null`,
  `scoreable: false`, never presents the card and is a pipeline exercise, never
  a measurement.
- **One attempt.** Every outcome — submitted, budget-exhausted or incomplete —
  writes `<workspace>/replays/<case_id>.json` at once, and that file's
  existence is the case's lock. A session lost to a crash or force quit leaves
  the case unlocked; the one-attempt rule is enforced administratively there.
- **Workspace export.** `scripts/phase9i_assisted_workspace.py` with
  `core/examples/phase9i_assisted_workspace.rs`; the workspace layout, the
  audit and the recorded program hashes are described in
  [the workspace status note](phase-9i-assisted-workspace-status.md).

## Scoring pins (2026-09-16)

The scorer is frozen here. Nothing in this section changes the population, the
gates or the pass rule above; it records what a scoring run's `predeclared.json`
must pin and what a scored case means.

- **Candidate.** `assisted_tiled_soft_prior_v1`, declared in
  [phase-9i-assisted-scoring-v1.json](../designs/phase-9i-assisted-scoring-v1.json): the
  unchanged V2 `tiled_soft_matting::estimate` with `SoftPriorInput { version: 1,
  user_constraints: Some(labels), prior_weight: 1.0 }` and
  `ClosedFormParams::default()`, no foreground protection. One kernel; no
  tuning. The reasons — the global solver's 500,000-unknown refusal, V2's
  measured 90-case run inside the 2 GiB harness, and `copy_coverage` making a
  labeled pixel exact — are recorded in
  [the scoring note](phase-9i-assisted-scoring-status.md).
- **Solver inputs.** `cases/<anon>/source.png`, `cases/<anon>/prior.png` and
  `replays/<anon>.json` only, with every declared PNG and decoded-plane sha256
  re-checked. The reference foreground is read for METRICS ONLY, and only after
  the workspace source and prior are proved byte-for-byte equal to
  `frozen::photo_source` and `frozen::prior`.
- **Constraints.** `InputPolicy::AssistedHumanLabelsV1` hands
  `assisted_replay::label_plane`'s categorical 0/128/255 plane to the kernel as
  hard `user_constraints`; the automatic trimap is NOT a constraint and its
  counterfactual audit is recorded separately as
  `legacy_automatic_constraint_audit`.
- **Replay admission.** Final only; human origin, `scoreable`, and a
  `source_only` attestation; matching `case_id`, `workspace_id`, extent, the
  four `displayed_assets` hashes and the task card; a recorded `label_plane`;
  and every per-event `label_plane_sha256` reproduced from the truncated session
  through the same rasterizer. Every recorded outcome — submitted,
  budget-exhausted, incomplete — is admissible.
- **Refusals are in the denominator.** A missing, malformed, unscoreable or
  hash-failed replay, and a case past the `256 * pixels + 64 MiB` harness
  admission, are scored refusal records, never skipped cases.
- **Exposure.** A declared-exposed case, or one whose replay reports prior
  exposure, is scored, counted and listed. It is never excluded.
- **Claim rule.** An assisted RESULT requires a non-dry run, no subset
  override, every completion-subset case present, and `qualified` under the
  chosen gate. A synthetic dry run is a pipeline exercise and claims nothing.
- **Tooling.** `scripts/phase9i_assisted_scoring.py` with
  `core/examples/phase9i_assisted_quality.rs`; the unchanged
  `scripts/phase9i_gate_v2_rescore.py` `score_run` makes every pass decision.

## Run freeze, resource policy and pass rule

Before scoring, make a new immutable run declaration pinning the prepared
78-case input manifest, completed replay manifest, every compared pipeline and
all source/package/weight hashes, preprocessing, RGB domain, constraint policy,
raw output/quantization rules, resource/work limits and refusal semantics.
Globally select comparisons before scoring; preserve V3 and V4 automatic results
as historical controls. Any assisted adaptation of them needs its own declared
pipeline identity under the same guidance; old automatic rows cannot be relabeled
as assisted measurements. Record a locked-prior/source control as diagnostic only.

Keep native extents and the existing full-image `256*N + 64 MiB` harness
admission under 2 GiB. This legacy reservation does not cover an arbitrary new
model automatically: forecast the **combined** peak for model state, inputs,
guidance, padding/tiles, inference, foreground reconstruction, metrics and
retained output overlap. Admit only when the declared combined reservation fits
2 GiB. Bound algorithm work independently and declare approximation/refusal
semantics. No concurrent large jobs without a combined forecast. Report estimates
and measured memory separately; a sampled monitor is not an OS allocation cap.

Each of the 78 declared cases must have an admitted output, valid bounded replay,
native extent, resource compliance and every applicable original quality gate:

- Boundary-alpha MAD regresses by at most `1/255`; soft-detail recall is at
  least 95%, using the original boundary and soft-detail populations.
- Source RGB at reference-opaque pixels remains exactly equal, including inside
  authorized correction regions. A footprint cannot exempt reference errors. (superseded for gate v2 scoring by the bounded opaque rule; see the amendment above)
- Alpha-weighted foreground MAD regresses by at most `1/255`.
- Fractional foreground MAD is at most 0.05; if its initial value exceeds 0.02,
  it improves by at least 20%.
- Boundary recomposition MAD is at most 0.02 separately on white, dark and
  checker. Missing fractional reference coverage cannot supply a color pass.
- Exact constraints, unchanged source alpha, unchanged matte at `A=0`, unchanged
  RGB at `C=0`, and exact captured RGB/matte in `P` all pass. Independently
  recompute feathered write equations as well as their zero-coverage guarantees.

All comparisons use the original captured `(I,M0)` as the before baseline, never
the human-adjusted mask or a postprocessed baseline. Refusals, annotation
mistakes, unfinished cases, budget violations, malformed outputs and missing
records remain in the denominator as failures. A case exhausting its budget
with a valid completed replay can still pass; an incomplete replay cannot.
Report alpha, foreground, combined, contract and resource results independently.
The whole-run pass requires **78/78** combined quality and contract/resource
passes, required category evidence, and independent recomputation of every gate.

Retain all original-sized outputs and refusal records. Inspect native contours
and opaque hair/skin/eyes/teeth, fur texture, product details and replacement
backgrounds; include artifact hashes and refusal reasons. Report candidate time
separately from imports, annotation, scoring and image export. If shared scoring
changes, independently establish legacy parity too. A changed implementation,
input policy or guidance collection creates a new frozen experiment, retaining
the earlier run; post-score tuning cannot alter the frozen result.

Passing v1 establishes only the declared assisted ordinary-edge experiment.
Typed estimator qualification, public UI/MCP/Actions parity, captured preview/
apply/Undo/persistence and original automatic completion requirements remain
separate work. General transmission/refractive claims require different evidence
and an explicit optical contract.
