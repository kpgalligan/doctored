# Phase 9I — joint Bayesian candidate V5

**V5 does not qualify the original cutout gate.** All 90 cases completed with
only two combined passes. No controls are enabled; 9I/P7-12 and Phase 9 remain open. This experiment follows
committed `d5ee109` and preserves
all four earlier declarations, results and original metric thresholds.

## Declared hypothesis

The earlier pipelines estimated alpha and then reconstructed foreground RGB
while holding that alpha fixed. A wrong alpha can therefore be absorbed into
foreground color: with exact background and positive estimated alpha, the error term is
`F_estimated - F_true = (alpha_true/alpha_estimated - 1) * (F_true - B)`.
V5 tests joint estimation instead of adjusting the old protection thresholds.

The [declaration](../designs/phase-9i-bayesian-v5.json) was frozen at
`2026-09-13T02:47:24.667919+00:00`, SHA-256
`77eeceb69a7f61a851cce95bf232e73afff65906a6e5bf606195bcdc6e59f143`.
Executable SHA-256:
`2b2a53d0c67b9188dc8e0afaa9a2140b302949bb20cd46024bf133eeb24c6f16`.
`~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-joint-bayesian-v5/predeclared.json`
records those hashes,
source hashes and both original reference-manifest hashes before scoring.

The Gaussian compositing objective follows
[Chuang et al., Bayesian Matting](https://grail.cs.washington.edu/projects/digital-matting/image-matting/),
with an explicit finite Gaussian alpha-prior term added by this experiment.
Source RGB and the automatic prior are its only benchmark inference inputs;
reference foreground, reference alpha and case names never enter its kernel.
The benchmark supplies all-Unknown user labels. Actual caller labels 0/255 alone
pin alpha exactly. Automatic endpoints are solved too, allowing detail absent
from a binary prior to change.

Automatic prior values at most 8 or at least 247 provide uncertain color support
only after excluding pixels less than 8 pixels inside the corresponding class's
contour. The existing Euclidean distance transform supplies that distance.
The support observations remain uncertain; they are neither user strokes nor
proof of physical opacity. The existing sampler provides eight nearest spatial
samples plus at most eight global palette colors, motivated by the distant-color
evidence in [He et al., Global Sampling](https://www.microsoft.com/en-us/research/wp-content/uploads/2011/06/HeEtAl.pdf).
V5 fits one inverse-distance-weighted full RGB covariance per class to those
samples. This single-distribution approximation and its boundary-only sample
population are explicit departures from full Bayesian mixture estimation.

The observation variance is the encoded-byte quantization floor
`1/(12*255*255)`, not calibrated camera noise. The same variance is added to each
sample covariance, following the authors' covariance correction. Alpha-prior
sigma is 0.5. At each tested alpha, conditional foreground/background estimates
come from a 3 × 3 Gaussian Schur complement using the existing SPD factorization.
The bounded search evaluates nine uniform grid points, the supplied prior, the
color-line projection and eight golden-section iterations: at most 21 objective
evaluations. The objective is recomputed after color clipping and includes the
foreground/background covariance normalizers; it is joint MAP, not a claim of
global convergence or physical confidence. Final foreground RGB is quantized
once. Output-alpha endpoints retain source RGB exactly.

## Admission and independent contract checks

The existing 2 GiB complete harness cap, native fixture dimensions and original
alpha/color gates are unchanged. V5 separately caps joint objective evaluations
at 200 million and measured spatial-tree node visits at 2 billion. These are separate
work units, not a claim that either number counts every CPU operation. The
existing 250,000-boundary-sample cap remains. There is no silent image resizing.
Missing foreground/background evidence and exhausted work budgets refuse the
whole candidate; unchanged-prior/legacy-color diagnostics cannot pass in its place.

`64*N+16MiB` candidate scratch covers output, shared distance-transform scratch
and the bounded sample-index storage; borrowed RGB/prior/label planes add 14N.
The unchanged full harness `256*N+64MiB` also encloses joint-F retention across
legacy oracle scoring: a conservative overlap is 201N + 1 MiB. Both resource shapes
are documented in the implementation and were reviewed independently.

Thirty-six distinct focused Rust tests pass across the numeric solver, source
support/driver, joint-output contract and old sampled/closed-form suites. They
include correlated weighted covariances, source-derived detail recovery, exact
explicit labels, malformed F extent/nonfinite/out-of-gamut refusal, an independent
joint-F compositing witness, and live cancellation during both joint estimation
and fully labeled copying. Live cancellation preserves inputs, returns within
the test's two-second bound and releases scratch. Logs:
`/private/tmp/rz-phase9i-v5-focused.log`,
`/private/tmp/rz-phase9i-v5-driver-final.log`,
`/private/tmp/rz-phase9i-v5-live-cancel-tests.log`.
All-target Clippy passes:
`/private/tmp/rz-phase9i-v5-clippy-verified.log`.

An independent 120-case legacy replay preserves every guided 24, sampled 48 and
closed-form 48 case field, gate and summary after removing only timing fields.
No missing/duplicate cases or source/executable changes occurred. Receipt:
`/private/tmp/rz-phase9i-continue-parity/parity.json`.

A pre-score review corrected a covariance-construction rounding bug by computing
one triangular entry and copying it to its mirror. A correlated-RGB regression
covers that contract. A later test-only lint cleanup replaced temporary fixed
vectors and a range spelling; scored inference/evaluator source and example
executables remain unchanged. These are correctness/hygiene fixes, not fitted
threshold changes.

## Results and remaining work

The original 90-case run is preserved at
`~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-joint-bayesian-v5/`.
Reference verification and both
benchmark stages exit 0; the strict wrapper exits 1 because the original combined
quality gate fails. Every original case, refusal and metric is retained.

| Corpus | Cases | Admitted joint outputs | Alpha passes | Combined passes | Refusals |
| --- | ---: | ---: | ---: | ---: | ---: |
| Analytic | 24 | 18 | 12 | 2 | 6 |
| Cropped photographs | 24 | 24 | 4 | 0 | 0 |
| Complete images | 42 | 18 | 3 | 0 | 24 |

The two combined passes are light-background analytic fur with blurred and
binary priors. All six analytic glass cases lack foreground color support;
all 24 complete portrait cases exhaust the declared spatial-node budget. No
image-size substitute is scored. The legacy unprotected reference-alpha color
rows remain 20/24, 22/24 and 30/42; they do not measure the joint candidate with
reference alpha and cannot qualify it.

The following overlapping failure counts include **admitted joint outputs only**.
The separate refusal-diagnostic values remain in the original reports and audits.

| Original criterion failed | Analytic /18 | Crops /24 | Complete images /18 |
| --- | ---: | ---: | ---: |
| Boundary-alpha nonregression | 6 | 15 | 14 |
| Soft-detail recall | 6 | 10 | 9 |
| Exact reference-opaque RGB | 16 | 24 | 18 |
| Weighted foreground nonregression | 0 | 14 | 18 |
| Fractional foreground error | 0 | 5 | 10 |
| Visible-fringe improvement | 0 | 3 | 16 |
| Replacement-background boundary error | 6 | 22 | 16 |

This is not an aggregate improvement over all prior candidates. Joint color
estimation works on some simple analytic populations, while cropped photographic
interiors and full-image resource admission regress. The original alpha and color
thresholds are unchanged in the [reference protocol](phase-6-matting-reference-protocol.md).

Independent recomputation checks every serialized alpha/color/combined result,
all 90 unique cases and all original formulas. Receipt:
`/private/tmp/rz-phase9i-continue-quality-audit.json` (purged from /private/tmp; not
preserved). Detailed overlapping reasons and separate admitted/refused
classifications are also retained in
`~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-v5-diagnostics/metric-audit-final.json`.
The wrapper took 1,421.56 seconds including verification, artifacts and scoring,
excluding builds. Candidate-call time totals 1,252.41 seconds, including refusals;
this is one research measurement, not production latency qualification. Admitted
calls reached 34,406,400 joint evaluations and 950,716,622 spatial-node visits.

Pinned reports:

- `run.json`: `dc48005a1dd3558fe4943c6eda885cbcb13c3270023c95a77118105264a33cab`.
- `analytic-crops/metrics.json`: `59f6de011c2679bc129f7caf75bca148123ae5e721f0dd21934ceec386749fcd`.
- `full/metrics.json`: `a5fd56beb61baf269e4c5f09ce3f35245894b3bc5dd9006bd45ff2ab8639d9a7`.

Post-hoc native-pixel audits already show the distinction between recovered
missing detail and damaged interiors. Light/binary fur recovers all 1,210 missing
soft-detail pixels with no changed opaque alpha or RGB. Light/binary portrait
recovers all 1,066 but changes 350 reference-opaque pixels (minimum alpha 253,
maximum RGB change 2 codes); product recovers 12/12 but changes 12 opaque pixels
(minimum alpha 254, maximum RGB change 1). Neither failure is exempted.

Four light/blurred photographic crops show much larger model error: 149,855–259,322
reference-opaque RGB pixels change, with maximum differences 122–225 codes. Most
lie inside the automatic foreground support itself. This is a structural sample
and model mismatch: V5 solves all image pixels using a boundary-only color
population inherited from an unknown-edge solver, and one Gaussian can explain
interior color variation as transparency. Increasing a numeric threshold would
not establish reliable foreground evidence.

Independent native-pixel inspection corroborates the numeric damage. The fur
light/binary dark-background panel restores a clean contour relative to the
prior's white fringe. The first light/blurred portrait has angular hair/forehead
patches, altered cheek/skin texture and darkened teeth against its reference,
including in the full 1280 × 1280 comparison. Files under the run's `analytic-crops/`
are `fur-light-binary-dark-native-panel.png`,
`photo-0051115Q_000001_0003-light-blurred-dark-native-panel.png`,
`photo-0051115Q_000001_0003-light-blurred-dark-estimated.png` and its
`-dark-reference.png` counterpart. This inspection corroborates the original
opaque/recomposition failures; it is not a new metric or gate.

The complete run also records 24 spatial-search budget refusals on full portraits.
Boundary populations and all-pixel queries stress the inherited exact
spatial tree. Refused artifact planes are unchanged-prior/legacy-color diagnostics,
not an admitted joint foreground result. Post-hoc audit scripts and pinned PNG
hashes live at `~/devel/rasterize-fixtures/phase9i-reports/rz-phase9i-v5-diagnostics/`;
reference-derived audit
values never return to candidate inference. `artifacts-final.json` contains 35
saved-case audits and explicitly separates 25 admitted outputs from 10 diagnostic
artifact sets belonging to refused cases.

A defensible next experiment would qualify spatially representative interior
color observations and multiple reliable foreground/background populations,
with explicit uncertainty and independently sourced user constraints where
available. A predeclared population design should sample interiors as well as
contours and represent distinct color modes. Any source-only correction replay
must be blinded to reference planes and recorded before scoring. It must preserve source detail while avoiding the present
interior-transparency failure, and bound its sample queries at full size. The
absence of opaque foreground evidence for analytic glass and of physical
photographic transmission truth remains separate. No result here establishes
that every future candidate must fail, or qualifies the current one.


## Reproduction

```sh
cd core
CARGO_TARGET_DIR=/private/tmp/rz-phase9i-next-target CARGO_PROFILE_RELEASE_LTO=false \
  CARGO_BUILD_JOBS=4 cargo build --release --example phase9i_bayesian_quality
cd ..
python3 -B scripts/phase9i_soft_prior_qualification.py \
  ~/devel/rasterize-fixtures/phase9i-reports/new-joint-bayesian-run --gate v2 \
  --bayesian --executable /private/tmp/rz-phase9i-next-target/release/examples/phase9i_bayesian_quality \
  --crops ~/devel/rasterize-fixtures/phase6/references \
  --full ~/devel/rasterize-fixtures/phase6/full-references
```

The output directory must be new. The runner freezes the declaration, executable,
source and original reference hashes before scoring; strict success requires every
original case to pass. Functional cutout assembly/cancellation and typed source
alpha/matte persistence have separate checks and do not waive this quality gate.
