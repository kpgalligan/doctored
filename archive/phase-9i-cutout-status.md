# Phase 9I — cutout quality research remains open

**9I is not complete.** Five predeclared candidates improve parts of
the original benchmark, but none passes its unchanged combined alpha/color
gate. Advanced brushing and decontamination remain disabled. No unqualified
candidate was promoted into typed-source refinement, UI, MCP or Actions.

**Gate rescoped.** The gate these five candidates were measured against is now
gate v1. [Gate v2](phase-9i-gate-v2-status.md) records the information-bound
analysis behind this document's failures, freezes the rescoped pass rule and the
bounded opaque rule, declares the 36-case assisted completion subset, and
re-scores all five candidates below under both gates. Every measurement and every
number in this document is unchanged by that rescoping.

The [joint Bayesian continuation](phase-9i-bayesian-v5-status.md) tests a different
alpha/foreground/background model while preserving the original gates and all
earlier results. The [execution continuation](phase-9i-execution-status.md)
adds cancellable byte/native preparation, strict single-use parent controls and
owner-close forwarding. Functional execution passes do not qualify cutout quality.

**Feasibility prerequisite decision:** the [next-step review](phase-9i-feasibility-status.md)
defines assisted refinement, audits the preserved evidence and selects RVM as one
conditional reference pipeline. No reviewed pipeline currently meets every
prerequisite, and no new candidate was scored. Exact checkpoint terms remain
unresolved; assisted scoring also needs an isolated workspace, tool and genuine
blinded human guidance. Resolve those dependencies before the one comparison in
the [execution plan](phase-9i-next-steps.md). Phase 10 proceeds independently.

## Evidence policy and shared implementation

`matting_input.rs` adds an explicit version-1 input seam: automatic prior bytes
are uncertain, including 0 and 255; optional caller-supplied categorical user
labels alone are exact constraints. Unknown (128) releases a user constraint.
Unknown versions, malformed labels, nonfinite weights and unsupported requests
refuse. No automatic threshold or erosion is reinterpreted as a user stroke.

The original closed-form entry point retains its categorical hard-label and
initializer-only prior contract. `estimate_soft_prior` instead adds
`lambda * sum((alpha - prior)^2)` to the same color-line Laplacian. Both paths
share stencil construction, PCG, true-residual confirmation, cancellation and
refusal. The positive prior term removes the nullspace even when a translucent
object has no opaque foreground sample. This establishes numerical solvability,
not physical evidence of foreground opacity or transmission.

The evaluator now has an explicit input-policy field. Old candidates retain
their old mode and every old output check. Soft-prior candidates receive
all-Unknown user constraints and the unchanged degraded prior; candidate code
does not read case names, reference alpha or reference foreground. Its output is
checked against actual user constraints. Each soft-policy record separately
retains the old automatic-label constraint audit for comparison. Saved `trimap`
PNGs retain those legacy automatic labels as diagnostics; they are **not** the
soft candidate's hard constraints. Independent metric bodies and gates are
unchanged.

The [original alpha/color gates](phase-6-matting-reference-protocol.md) still
apply to every case: boundary-alpha MAD may regress by at most `1/255`, and
soft-detail recall must be at least 95%. Source RGB at reference-opaque pixels
must remain exact; alpha-weighted foreground MAD may regress by at most `1/255`;
fractional foreground MAD must be at most 0.05 and improve at least 20% when its
initial value exceeds 0.02. Recomposition boundary MAD must be at most 0.02 over
each of white, dark and checker backgrounds. A refusal cannot pass, and a
reference without fractional coverage supplies no color pass. Combined success
requires both alpha and color qualification, plus the original photographic
inspection/evidence requirements.

## Predeclared candidates and complete results

Each declaration and executable/source/reference-manifest hash was written to
`predeclared.json` before that candidate was scored. Every case, refusal, numeric
metric and original artifact is retained in a new external directory. Parameters
were not changed within a run; no reference-derived correction replay was used.

| Candidate | Analytic alpha /24 | Crop alpha /24 | Full-image alpha /42 | Combined analytic / crop / full | Refusals analytic / crop / full |
| --- | ---: | ---: | ---: | --- | --- |
| Original sampled, preserved | 10 | 8 | 15 | 0 / 0 / 0 | 6 / 0 / 0 |
| Original closed-form, preserved | 10 | 9 | 15 | 0 / 0 / 0 | 6 / 0 / 0 |
| Soft prior v1 | 13 | 0 | 0 | 1 / 0 / 0 | 0 / 24 / 42 |
| Tiled soft prior v2 | 12 | 12 | 20 | 1 / 0 / 0 | 0 / 0 / 0 |
| Source-protected tiled v3 | 12 | 12 | 20 | 1 / 3 / 4 | 0 / 0 / 0 |
| Source-protected stable prior v4 | 12 | 12 | 18 | 0 / 3 / 4 | 0 / 0 / 0 |
| Joint Bayesian alpha/color v5 | 12 | 4 | 3 | 2 / 0 / 0 | 6 / 0 / 24 |

V1 uses `lambda=0.01`, epsilon `9/(12*255*255)`, at most 500 PCG iterations and
RMS residual tolerance `1e-6`. Every pixel participates in one global solve.
It admits all analytic cases, including the six formerly refused glass cases,
but the unchanged 500,000-unknown and 20-billion-stencil-term caps refuse all
photographic cases. Refused cases retain unchanged-prior/color diagnostics,
never an admitted candidate output or a quality pass. Its one combined pass is
portrait/hair over light with the binary prior.

V2 addresses that measured resource failure using native-resolution tiles:
256-pixel output squares with a 16-pixel context halo, `lambda=1`, the same
epsilon/tolerance and the same Laplacian/PCG. Each tile's central samples are
copied exactly once, without resizing or overlap blending. Local convergence is
not a claim of global-system convergence. The complete call counts every
repeated halo pixel against the original 20-billion-term work allowance and
reduces its iteration ceiling from 500 based only on dimensions. A single
unconverged tile refuses the whole image; no partial result qualifies.

All 90 v2 cases finish at their original sizes. Full-image iteration limits are
15, 102, 233 or 268 depending on dimensions; the largest admitted work estimate
is 19,994,875,200 terms. The maximum measured local true residual is
`9.997365469789079e-7`. The 42 full-image alpha calls total 111.36 seconds on this
machine, excluding foreground estimation, scoring, artifacts and builds; this
is one run, not a production latency claim. The existing 2 GiB harness allowance
is unchanged. Its one combined pass is hard product over light with the binary
prior, a different case from v1, so neither candidate dominates the other.

V3 keeps every v2 alpha result and tests a separately declared source-only RGB
protection policy. Color corrections are eligible only when supplied alpha is
1–127, the automatic prior is below 128 and the user label is Unknown. Nearby
observed foreground colors (prior at least 247 or an explicit user foreground
label) within a radius-16 square protect a pixel when all RGB channels are
within eight byte codes. Explicit background never supplies foreground support.
Outside that footprint, source RGB stays exact. A 250-million-neighbor-check cap
is admitted before output allocation. These parameters were fixed before the
run; they do not classify opacity with certainty.

V3 reduces opaque-RGB failure counts to 3/24 analytic, 0/24 crop and 3/42 full,
but retains more contamination: replacement-boundary failures increase to
22/24, 21/24 and 33/42, and fractional-color failures to 11/24, 13/24 and 24/42.
Its full combined passes are the three blurred cat recompositions and one
dark/blurred portrait. Its three crop passes are dark/blurred portraits. There
is no case-specific policy or reference-derived correction input.

V4 tests whether avoiding alpha-solver regression resolves the remaining color
limit. It preserves the supplied prior exactly except for explicit user labels;
the benchmark supplies none. All 90 before/after alpha metric objects are
identical. V3's source-only foreground protection/reconstruction is unchanged.
All binary starts still fail the original soft-detail recall gate, as do the
three full bottle blurred starts; a stable prior is not automatically a pass.
V4 changes opaque RGB at six pixels in each blurred analytic product and 27–28
pixels in each blurred full bottle. Every portrait crop preserves opaque RGB,
but 20/24 crops still exceed replacement-boundary MAD and 14/24 exceed fractional
foreground MAD. Full failures remain 32/42 replacement boundary, 25/42 fractional
foreground and 25/42 insufficient visible-fringe improvement. These identify
missing input detail and residual foreground evidence, independently of solver
regression. Another arbitrary threshold is not justified by these measurements.

Declaration SHA-256:

- [V1](../designs/phase-9i-soft-prior-v1.json): `c7e34a171710a4544cc5eabc4e168df2dc7286f0cdcbd2eb433735311c9da7ed`;
  frozen at `2026-09-12T23:27:39.470623+00:00`.
- [V2](../designs/phase-9i-soft-prior-v2.json): `de1979d937282ae5d7fcc025693050934bde289168ef9f3977d3a2459f45e39e`;
  frozen at `2026-09-12T23:33:34.653223+00:00`.
- [V3](../designs/phase-9i-soft-prior-v3.json): `613f57b644c9f5a4b30bf05744aa1f1630aae31312456db73dba6b6de66b1f5b`;
  frozen at `2026-09-12T23:46:57.980256+00:00`.
- [V4](../designs/phase-9i-soft-prior-v4.json): `998344695e02e76cf16fd7322b5fad440070b1359f6107b9bf6121a292d05851`;
  frozen at `2026-09-12T23:59:19.995115+00:00`.
- [V5](../designs/phase-9i-bayesian-v5.json): `77eeceb69a7f61a851cce95bf232e73afff65906a6e5bf606195bcdc6e59f143`;
  its [complete qualification](phase-9i-bayesian-v5-status.md) preserves all 90
  cases. The two combined passes are analytic light-background fur. All 24
  photographic crops fail the combined gate; all 24 full portraits refuse the
  predeclared spatial-search work limit. Other full images are evaluated and
  fail quality. Missing foreground support still refuses six analytic glass
  cases. The frozen all-pixel model damages opaque photographic interiors;
  its simple-scene improvements do not qualify advanced controls.

## Remaining quality failures

V2 changes reference-opaque RGB in 17/24 analytic, all 24 crop and all 42 full
cases. Independently decoded saved PNGs confirm maximum opaque-channel changes
of 69 bytes across the analytic artifacts and 32 bytes across the four saved
light/blurred portrait crops. Other photographic byte deltas were not separately
decoded; their complete metric counts remain present.

Failures extend beyond exact opaque preservation. V2 exceeds replacement-
background boundary MAD 0.02 in 20/24 analytic, 14/24 crop and 30/42 full cases;
fractional foreground MAD exceeds 0.05 in 9/24, 8/24 and 20/42 respectively.
These failure counts overlap. The preserved `oracle_alpha_color` rows measure
the **legacy unprotected** multilevel estimator supplied with reference alpha:
20/24, 22/24 and 30/42. They do not measure the protected v3/v4 pipeline and cannot
qualify its output. A protected reference-alpha variant was not measured. Later
reports label this pipeline distinction explicitly without altering old records.

Native-pixel panels show a residual halo at the first portrait's outer hair
over a dark replacement background. Binary-start analytic glass still loses
most interior transmission despite having a numerical solution. The passing
hard-product/light/binary example preserves its antenna and small hole. These
inspections support the recorded per-case limits; a pleasing example does not
waive any other case.

The [foreground ambiguity evidence](phase-6-foreground-evidence.md) remains
applicable: fitting a compositing equation cannot prove that an unknown pixel
is opaque. The old hard-label audit only proves a limitation of those frozen
inputs; it does not prove every future candidate must fail this finite corpus.

Additional primary-source checks did not yield a suitable new photographic
transmission reference. [Transparent-460](https://github.com/AceCHQ/TransMatting)
still requires author permission for commercial use. No contact or agreement
was made. [alphamatting.com](https://alphamatting.com/datasets.php) offers
16-bit linear foreground/input data, but dataset permission was not established;
it also discloses noisy differences between its measured opaque foreground and
input RGB. Those data cannot silently replace the current encoded-RGB gate.
[BackgroundMattingV2](https://github.com/PeterL1n/BackgroundMattingV2) requires an
additional captured background image. Such a workflow is a concrete source of
stronger evidence, but no independent background capture is available here;
constructing extra observations from reference alpha/colors would contaminate
candidate inference. The existing bottle remains silhouette truth only.

## Internal native output foundation

`precision_cutout.rs` follows the existing byte cutout's top-root insertion and
original-root hiding rules. It shares every supplied foreground RGBA16 sample,
including source alpha 1, partial alpha and hidden RGB, then copies an independent
canvas-sized u16 refinement matte into a normal editable mask. It never flattens
the alpha product into one plane. A tiny positive source-alpha/matte product is
retained even when a later display render rounds below one code; replacing the
mask with full coverage reveals the exact retained source alpha again.

The additive plan/edit FFI and `RasterPrecisionDocument` wrappers keep RGBA16
and borrowed u16 ownership explicit. Plans include unique retained document/
foreground storage, borrowed matte bytes and separately allocated mask/record
scratch. Exact equal-byte profiles rebind to the captured document profile owner
without conversion; mismatched profile, extent, storage, capacity or budget
refuses. Empty effective coverage is a no-op. Old groups, masks, channels,
descriptions, retained originals and metadata survive hidden on their old roots.
The current native schema already represents this state; no format bump occurs.

`NativeRefinementCutout.swift` uses the same captured document/selection and
trimap session guards as byte output. Capture no longer materializes an unused
byte projection. Assembly and native display preparation are separately admitted;
publication checks revision/session/cancellation and now closed-owner state,
then uses the existing central one-edit Undo/Redo path. The native display guard
requires the typed projection, not the byte projection field. The subsequent
[execution continuation](phase-9i-execution-status.md) controls native rendering
and byte-preview conversion through fresh child tokens. The byte twin uses the
shared compositor in bounded chunks and controlled record/string copying.
Metadata parsing is admitted before execution; individual JSON parses,
CoreGraphics calls and bounded preflight retain their documented cancellation
boundaries. This is not qualification of a public advanced workflow.

This foundation is exercised internally only. Current alpha/color estimators
still require their declared opaque, encoded-RGB inputs. They have not been
silently promoted into a qualified RGBA16/partial-source-alpha estimator. The
initial isolated worktree used the 9F baseline. Integration over the qualified
9G/9H commits reuses the current document/target/selection guards and accepts
`PrecisionScalarBuffer` with its registered payload lease, counting borrowed
matte samples once. The integrated host probe also exercises exact native
selection coverage through preparation, Undo/Redo, snapshot and recovery.

## Continuation qualification

The joint-estimator and execution continuation passes its functional integration
checks. The original quality gate still fails; this is not completion of 9I.

- Full Rust regression: **1,410 passed, 0 failed, 2 existing ignored**, across
  145 result groups. All-target Clippy, formatting and diff checks pass. Logs:
  `/private/tmp/rz-phase9i-continue-full-rust.log` and
  `/private/tmp/rz-phase9i-continue-clippy.log`. All **432 core/test/example
  inputs** remain unchanged after these checks; subsequent fixes affect Swift
  scheduling and borrowed-input accounting only.
- Final authoritative `make typecheck` passes with **0 errors**:
  `/private/tmp/rz-phase9i-execution-typecheck-final.log`. The final AppKit probe
  passes **6 checks / 15 groups**, including six actual Cancel/owner-close core
  checkpoints across byte projection, native projection and native preview;
  progress-panel factory and nested-busy cleanup; exact borrowed/registered
  matte accounting; and source/mask/Undo/native/snapshot/recovery/gate checks.
  All **691 source/helper inputs** remain unchanged. The
  [execution receipt](phase-9i-execution-status.md) records individual observations,
  hashes and cancellation boundaries. Log:
  `/private/tmp/rz-phase9i-execution-final-qualified-probe.log`.
- Final normal-LTO `make app` passes. All **674 production inputs** match
  `/private/tmp/rz-phase9i-continue-production-freeze-v2.json`; build log:
  `/private/tmp/rz-phase9i-continue-production-build-v2.log`. The immutable app is
  `/private/tmp/rz-phase9i-continue-final-candidate/Rasterize.app`, executable SHA-256
  `d24fcf9d377c8704f606ea9cc753967b736d7b6a0438490f1cd630a3981df0a3`;
  production core archive SHA-256
  `9e6ae7be7d06505c26739ae8ca3d8ee4cee623a37a47517123c424cb4937173e`.
- That exact production app passes all seven external MCP checks: native reopen,
  unchanged foreground RGBA16/source alpha and hidden originals, separate u16
  matte, save/edit/Undo/Redo, mask-to-selection-to-channel transfer and native v25
  channel options/reopen. Exact channel samples remain
  `[1, 257, 32771, 65534, 123, 65535]`; the reopened native file matches the earlier
  expected SHA-256 `68ba60ddeccec1ea64866033dc4cbc65d8a5ba97a7eeb971a0f220d93ad693f6`.
  Receipt: `/private/tmp/rz-phase9i-continue-final-release-native/result.json`.
  Owned test apps exited; tests used dummy credentials and private state.
- Independent replay preserves all **120 legacy case records** exactly apart
  from timing. An independent audit agrees with every original gate decision
  across **all 90 V5 cases**, including refusals. Receipts:
  `/private/tmp/rz-phase9i-continue-parity/parity.json` and
  `/private/tmp/rz-phase9i-continue-quality-audit.json`. The complete failed
  candidate evaluation remains in the [V5 status](phase-9i-bayesian-v5-status.md).

No public tool, Actions schema or native format changes. P7-12 and Phase 9 stay
open; eighteen carryovers are verified and 18 remain open.

## Committed d5ee109 integration milestone

The initial continuation was integrated over qualified 9G (`f457cdb`) and 9H
(`93a45cf`), preserving their additive u16 ABI, native selection and paint behavior.
These preserved milestone receipts do not replace the subsequent continuation
checks below or close the failed alpha/color quality gate.

- Focused integrated Rust: **27 passed, 0 failed** across the six relevant
  candidate/native suites and the shared null-safety test. Logs:
  `/private/tmp/rz-phase9i-integrated-focused-rust.log` and
  `/private/tmp/rz-phase9i-integrated-null.log`. `cargo fmt --check` and diff
  whitespace checks pass.
- Milestone authoritative `make typecheck`: **0 errors**, exit 0. Log:
  `/private/tmp/rz-phase9i-integrated-typecheck.log`.
- Milestone-source AppKit probe: **3 checks / 6 groups passed**, exit 0. The native
  path accepts the registered `PrecisionScalarBuffer`, retains exactly its
  original sample lease plus the new document metadata payload, and preserves
  genuine `.mask16` selection codes through preparation, commit, Undo/Redo,
  named snapshot save/load and private recovery write/read. Independent source
  alpha/matte and native reopen checks, the byte cutout regression and disabled
  advanced controls pass in the same run. Log:
  `/private/tmp/rz-phase9i-integrated-source-probe-final.log`; artifacts:
  `$TMPDIR/rasterize-selection-probe-2lxirdjk`.
  This probe links the isolated-target library with Rust LTO disabled; the
  separate production checks below use normal Release LTO.
- Full integrated Rust regression: **1,386 passed, 0 failed, 2 existing ignored**
  across 142 result groups. Required all-target Clippy passes. Logs:
  `/private/tmp/rz-phase9i-integrated-full-rust.log` and
  `/private/tmp/rz-phase9i-integrated-clippy.log`.
- Normal-LTO `make app` passes, including Release Swift code generation/linking.
  All **670 production inputs** remain unchanged from the recorded build capture:
  `/private/tmp/rz-phase9i-integrated-production-freeze.json`. Build log:
  `/private/tmp/rz-phase9i-integrated-production-build.log`. Executable SHA-256:
  `d4368ef1481ee17fa3a5259694b85955bad0cd27522fcfa0b8625a9b2dca94d5`;
  core archive SHA-256:
  `74518bb519450a7c1bb600d78e02bc253e820831bda24291980b7e96b3078a59`.
- The owned production app passes exact source RGBA16/alpha, independent matte,
  hidden-original, save and central Undo/Redo checks. Its cutout mask also passes
  through 9G's public mask-to-selection-to-channel workflow with exact codes
  `[1, 257, 32771, 65534, 123, 65535]`. Nondefault channel color/opacity/side and
  all original foreground, mask and channel samples survive **native v25** reopen.
  The owned process exited. Receipt:
  `/private/tmp/rz-phase9i-integrated-release-native/result.json`.

The two preceding integrated probe attempts found test-helper mistakes (accessing
an unmaterialized recovery state and omitting newly retained inspection metadata
from an accounting expectation). Both logs remain preserved; no production code
or quality threshold changed in those corrections.

## Verification and reproduction

The measurements below are preserved **isolated 9F-baseline receipts**. They
do not replace the committed G/H/I integration milestone above or the later
continuation qualification linked from this status.

Focused Rust checks cover independent closed-form quadratic-prior oracles,
one-pixel/no-class solvability, exact user-label override and release, strict
version/input refusal, native tile extent/assembly, bounded admission and
cooperative cancellation without partial publication. Existing general
closed-form dense-oracle and legacy constraint tests remain in the same run.
Four native cutout FFI tests additionally verify independent equal-byte profile
owners, tiny source alpha, a separate u16 matte, unchanged hidden originals,
native reopen, mask-only translation, channel reuse and strict no-op/error/cancel
purity, including live cancellation with no partially published native mask.
Three source-protection tests independently cover its exact footprint,
color-support thresholds, explicit user overrides, work admission and cancellation.

- Full Rust regression: **1,336 passed, 0 failed, 2 existing ignored**, across
  137 result groups. The final four-test native check strengthens independent
  profile-owner coverage and adds one live-cancellation test, for **1,337 distinct
  passing tests** across the full run and final focused checks. Logs:
  `/private/tmp/rz-phase9i-core-tests-final.log`,
  `/private/tmp/rz-phase9i-core-summary.json`,
  `/private/tmp/rz-phase9i-native-profile-tests-final.log`.
- Formatting, diff whitespace and all-target Clippy pass. Clippy log:
  `/private/tmp/rz-phase9i-clippy-verified.log`. Authoritative `make typecheck`
  passes with **0 errors** using isolated Swift/Clang caches:
  `/private/tmp/rz-phase9i-typecheck-verified.log`.
- The actual AppKit source probe passes **3 checks / 6 groups**: internal typed
  assembly, existing byte assembly and disabled advanced controls, including
  source-alpha/matte separation, before/after preparation, one clean Undo/Redo,
  stale/session/cancel/no-op/closed-owner refusal, native reopen, named snapshot
  save/load and the real private recovery writer/reader. It uses owned windows,
  dummy credentials and private Actions/recovery/RAW/filter state. Log:
  `/private/tmp/rz-phase9i-source-probe-persistence-final.log`; artifacts:
  `$TMPDIR/rasterize-selection-probe-6rrxgqyb`.
- `make app` passes in the isolated worktree, including Release Swift code
  generation/linking. The requested isolated Rust setting disables release LTO;
  app optimization/source settings were not changed. Log:
  `/private/tmp/rz-phase9i-app-build-final.log`. The resulting owned Release app
  passes an external MCP native reopen/save/edit/Undo/Redo check with independently
  decoded exact RGBA16 and u16-mask samples. Receipt:
  `/private/tmp/rz-phase9i-release-native-verified/result.json`. App SHA-256:
  `be1b9086edf470cd2f050a563ca95b5d41ad4ccd33f1fa0a357cc0041147dc89`.
- Independent review found no blocking native alpha/purity/history issues. Its
  oracle-pipeline interpretation note is documented above; protected reference-
  alpha behavior remains explicitly unmeasured. Functional verification does
  not change any failed numerical gate or enable advanced controls.

Exact replay comparison preserves all existing per-case metric/pass fields of
24 guided, 48 sampled and 48 original closed-form records, excluding timing and
the newly introduced diagnostic audit absent from the oldest sampled report.
The original guided count remains 12/24. All four earlier strict runs exit 1 for
measured numerical gate failure; all acquisition and benchmark stages exit 0.

Artifacts and reports:

Preserved reports live under `~/devel/rasterize-fixtures/phase9i-reports/`:

- `rz-phase9i-soft-prior-v1/`: declaration, stage logs, all 90 records,
  original-sized artifacts and independent failure diagnostics.
- `rz-phase9i-soft-prior-v2/`: the same complete evidence plus an
  independent PNG-header/hash audit of all 28 saved full-image source, alpha and
  foreground planes. All match their seven native reference dimensions.
- `rz-phase9i-soft-prior-v3/` and `-v4/`: separately frozen source-only
  foreground protection and stable-prior comparisons, each with all 90 records
  and independent failure-reason reports. V4 has exact alpha before/after parity.
- `gate-v2/rescore.json` and `gate-v2/rescore.md`: the v1/v2 re-scoring of all five
  candidates described in [gate v2](phase-9i-gate-v2-status.md).
- `/private/tmp/rz-phase9i-parity-final/parity.json` (purged from /private/tmp; not
  preserved): pinned old/current reports and exact legacy replay comparison.
- `/private/tmp/rz-phase9i-stability-audit/audit.json` (purged from /private/tmp; not
  preserved): exact v2/v3 alpha parity, exact v4 before/after alpha parity, and
  independent re-evaluation of the original alpha gate for every case.

```sh
cd core
CARGO_TARGET_DIR=/private/tmp/rz-phase9i-target CARGO_PROFILE_RELEASE_LTO=false \
  CARGO_BUILD_JOBS=4 cargo build --release --example phase9i_soft_prior_quality
cd ..
python3 -B scripts/phase9i_soft_prior_qualification.py \
  ~/devel/rasterize-fixtures/phase9i-reports/new-soft-prior-run --gate v2 \
  --executable core/target/release/examples/phase9i_soft_prior_quality \
  --crops ~/devel/rasterize-fixtures/phase6/references \
  --full ~/devel/rasterize-fixtures/phase6/full-references
# Add --tiled for the separately declared v2 candidate; use another new output directory.
# --protect selects v3; --stable-prior selects v4. Each requires a new output directory.
python3 -B scripts/selection-probe/verify.py \
  --checks 'native refinement cutout output,refinement cutout output,advanced refinement gate' --keep
python3 -B scripts/verify_phase9i_native.py --app build/Build/Products/Release/Rasterize.app \
  --source /path/to/source-probe/files/native-refined-cutout.rz --output /tmp/new-native-cutout-run
```

9I/P7-12 stays open. Remaining work is explicit:

1. Recover missing input detail with a defensible automatic method or genuinely
   blinded user-constraint workflow. All binary supplied priors fail the original
   recall gate even when no solver changes them. Inputs must never be repaired
   from reference alpha or foreground.
2. Improve source-only foreground reconstruction or add independently captured
   evidence. V3/v4 reduce opaque damage but preserve too much contamination;
   neither a residual nor local color similarity establishes physical opacity.
   Their finite-corpus failures do not prove every future candidate impossible.
3. Obtain appropriately licensed photographic transmission/foreground reference
   evidence. A silhouette-only glass annotation does not supply it.
4. Qualify the actual typed RGB/alpha estimator and connect it to the tested
   independent-mask foundation and 9G coverage tools. Before/after, full mask
   transforms, UI/MCP/Actions and complete execution/persistence checks remain
   required before exposure. Passing 9G or internal assembly tests does not
   waive quality, image-evidence or public-workflow requirements.
