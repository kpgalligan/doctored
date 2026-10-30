# Phase 9I — established pipeline feasibility decision

## Closing decision (2026-09-15)

**RVM is dropped and no pipeline is selected.** Issue #281 on the RVM repository
(the exact pretrained-weight terms) is still open and unanswered; the code is
GPL-3.0 while Rasterize is unlicensed (all rights reserved), so linking it is
impossible regardless of any weight grant; and RVM is trained on human video and
is trimap-free, so it cannot consume brush guidance. No neural matting model is
added; Apple Vision stays the automatic prior. The three-candidate survey, the
fourteen reviewed source hashes, the release-asset facts and the FBA/BGMv2
findings below stand as history. The gate is rescoped once in
[gate v2](phase-9i-gate-v2-status.md).

**No candidate is eligible for a scored run yet.** RVM MobileNetv3 is the single
preferred conditional pipeline after reviewing three established joint
alpha/foreground systems. Its exact pretrained-weight permission remains
unresolved under the work-package-1 prerequisite. This is not a finding that
evaluation is prohibited. No weights were downloaded, packages installed, model
loaded, inference run or publisher contacted. The machine-readable
[acquisition/runtime proposal](../designs/phase-9i-established-pipeline-manifest.json) is
explicitly **not a frozen candidate declaration**.

This decision follows [the restart plan](phase-9i-next-steps.md) at repository
baseline `19b7928`. The [ViTMatte spike](phase-6-vitmatte-spike.md) was read first:
both normalization variants failed every combined analytic case; its native
full-image attempt admitted 0/42 outputs under 2 GiB, and the automatic hard
trimap imposed impossible detail-recall ceilings on six analytic cases. No
normalization retry, larger cap, truth-derived labels or custom V5 successor is
proposed here. Existing failed results and all original gates remain intact.

## Three candidates reviewed

| Complete pipeline | Inputs and author processing | Separate rights evidence | Decision |
| --- | --- | --- | --- |
| **Robust Video Matting, MobileNetv3 + author deep guided filter** | RGB alone; predicts foreground RGB and alpha jointly. Human-video training domain. Author internal coarse inference plus refinement against the native source. Independent still images use cleared recurrent state. | Source is explicitly GPL-3.0. Official pretrained assets and their intended inference use are documented, but no distinct permission statement for the exact weight asset was established. | Preferred conditional pipeline. Resource forecast supports a bounded attempt; rights, artifact pinning and runtime/output qualification remain prerequisites. |
| **F, B, Alpha Matting** | RGB and a foreground/background/unknown trimap; predicts all three planes. Author demo rescales to multiples of 8 using Lanczos, adds ImageNet normalization and distance-transform guidance, restores native dimensions and enforces known labels. Composition-1k/Adobe matting domain. | MIT source; author explicitly restricts supplied Adobe-trained weights to noncommercial use/distribution. That restriction cannot be replaced with the source license. | Not selected. No accepted source-only user trimap exists; treating automatic prior endpoints as certain repeats the known input defect. Author recommends at least 11 GB GPU memory for Composition-1k, so native 2 GiB eligibility is not established either. |
| **Background Matting V2, MobileNetv2 + author refinement** | Native RGB plus a separately captured empty background, both normalized to [0,1]; coarse model and bounded sampled-patch refinement return native alpha/F. Human background-replacement workflow. | Author describes the project as MIT; no separate exact-asset grant was audited because the input prerequisite already fails. Dataset permissions remain separate. | Not selected. The frozen corpus has no independent empty-background captures. A benchmark compositing background or one reconstructed from reference alpha/F would be an unauthorized extra inference input. |

Primary sources: RVM [author repository](https://github.com/PeterL1n/RobustVideoMatting),
[inference contract](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/documentation/inference.md)
and [source license](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/LICENSE);
FBA [model terms and memory guidance](https://github.com/MarcoForte/FBA_Matting#models),
[demo](https://github.com/MarcoForte/FBA_Matting/blob/master/demo.py),
[transforms](https://github.com/MarcoForte/FBA_Matting/blob/master/networks/transforms.py)
and [code license](https://github.com/MarcoForte/FBA_Matting/blob/master/LICENSE);
BGMv2 [author overview and license statement](https://github.com/PeterL1n/BackgroundMattingV2)
and [input/refinement contract](https://github.com/PeterL1n/BackgroundMattingV2/blob/master/doc/model_usage.md).

## Conditional RVM input and output contract

This proposal evaluates a different complete model, including its learned
foreground residual. It would not attach the old foreground estimator to new
alpha. The [author model](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/model/model.py)
adds the predicted residual to source RGB and clamps F and alpha to [0,1]. The
[encoder](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/model/mobilenetv3.py)
performs ImageNet normalization internally; the host must not normalize twice.

Proposed adapter policy, to be frozen only after the prerequisites below:

- Keep all original 90 case identities, native dimensions, source composites,
  degraded priors, background variants and reference pins. Compare globally with
  preserved V3 and V4 unchanged-alpha control, alongside earlier sampled and
  closed-form records. Repeated priors remain separate benchmark rows even though
  an RGB-only model will produce identical outputs for identical source bytes.
- The model receives only native RGB8 converted to float32 RGB in [0,1] by
  division by 255, as `[1,3,H,W]`. The source's existing encoded-RGB domain is
  retained. No profile transform, linear-light reinterpretation, external resize,
  padding, automatic categorical labels or reference input is introduced.
  The prior is retained as comparison/provenance data but is not consumed by RVM.
- Use CPU float32, evaluation and inference mode, one independent still image
  per subprocess, batch size one, four intra-operation threads and one inter-op
  thread. Initialize all four recurrent inputs to `None` for each case; do not
  recycle state across different subjects, backgrounds or priors, or repeatedly
  feed a still image to improve its output.
- Use the author's [automatic ratio](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/inference.py):
  `min(512 / max(H,W), 1)`, chosen from geometry alone. Internal bilinear sampling
  uses the author flags; its [deep guided filter](https://github.com/PeterL1n/RobustVideoMatting/blob/53d74c6826735f01f4406b5ca9075eee27bec094/model/deep_guided_filter.py)
  consumes the full native source and restores native outputs. This is the
  declared author's multiscale algorithm, not a resized benchmark substitute.
- Validate emitted native alpha/F shapes, float32 channels, finiteness and [0,1]
  range before adapter conversion. Preserve raw emitted float planes as external
  evidence. Proposed benchmark byte conversion is `floor(255*x+0.5)` for alpha
  and each straight foreground channel, with no added clamp, cutoff, source-RGB
  protection, color reconstruction or manual-label override. This rounding is
  an explicit benchmark adapter rule; it does not claim equality with the
  author's image writer. Quantization must be checked independently before any
  quality score. Invalid outputs or resource failures refuse the entire pair.

The model has no assisted-label, protection or local-write interface. An RVM
automatic output therefore cannot qualify the separate assisted contract.
Introducing those guarantees would require a separately declared adapter and
the same frozen, source-only accepted guidance across comparisons. Human portrait
training is not evidence for fur, products or glass: all those original cases
would still be evaluated and any failures retained. This still-image experiment
would also make no temporal-video quality claim.

## Resource forecast, not measured admission

The original harness cap remains 2,147,483,648 bytes. Proposed orchestration has
one model child at a time, then reaps it completely before starting the scorer.
No runtime, decoded image, output tensor or model state remains in the controller;
external files carry the alpha/F pair between stages. Parallel model jobs and
model/scorer overlap are outside this proposal.

Let `N=W*H`, `s=min(512/max(W,H),1)` and
`L=floor(W*s)*floor(H*s)`. Proposed model planning is
`768 MiB + 128*N + 1024*L`, plus a separate **32 MiB** stdlib controller budget.
The native term accounts for source/decode, guided-filter tensors, emitted
foreground/alpha, validation and sequential byte export. At the guided-filter
expression, source RGB is 12N bytes and the native guide, coefficients A/b,
multiply intermediate and output each occupy 16N: roughly 92N live before
later clamping/export overlap. The remaining native allowance, low-resolution
term and fixed allowance cover additional copies, recurrent/feature storage,
weights, runtime, allocator retention and operator workspace. These allowances
are a source-informed planning estimate, **not a proven allocation bound**.

| Original extent | Cases | Stage-1 extent | Model + controller forecast | Scorer + controller forecast |
| --- | ---: | --- | ---: | ---: |
| 320 × 256 | 24 analytic | 320 × 256 | 933,232,640 B | 121,634,816 B |
| 1280 × 1280 | 24 crops | 512 × 512 | 1,317,011,456 B | 520,093,696 B |
| 763 × 764 | 6 full fur | 511 × 512 | 1,181,387,264 B | 249,893,888 B |
| 800 × 600 | 6 full bottle | 512 × 384 | 1,101,627,392 B | 223,543,296 B |
| 944 × 1338 | 6 full product | 361 × 512 | 1,189,801,984 B | 424,009,728 B |
| 2304 × 3456 | 24 full portraits | 341 × 512 | 2,036,858,880 B (1.897 GiB) | 2,139,095,040 B (1.992 GiB) |

Scoring retains the original `256*N + 64 MiB` envelope, adding 32 MiB for the
controller. The largest row leaves only **8 MiB** under the aggregate cap. A
future joint-output import must prove its raw-plane validation/decoding and
retained F fit that existing envelope; this is not already established by the
old alpha-only external adapter. Stage-local outputs must be released before
later artifact generation. A larger controller, import overlap or workspace
requires refusal or a newly reviewed implementation, not a higher memory cap.

Prospective resource checks include every owned process's RSS, parent/child
overlap, the child's exit-time `ru_maxrss`, observed controller peak, source and
output hashes, and actual dimensions. Polling at 50 ms is only a rejection
threshold and can overshoot; post-exit checks also reject a brief missed peak.
The unchanged proposed per-case wall limit is 120 seconds including startup and
export. Native work is linear in N; network/recurrent work is bounded by one
forward pass with both coarse dimensions at most 512. This gives an algorithmic
work shape, not a measured CPU-time or operation-count guarantee. There is no
iterative convergence loop or per-case policy search.

## Acquisition, runtime and missing prerequisites

Author source revision is `53d74c6826735f01f4406b5ca9075eee27bec094`.
Fourteen bounded source/document files and GitHub release metadata were fetched
for static review only to
`/private/tmp/rz-phase9i-rvm-source-audit-5y3j4w4d/` (purged from /private/tmp; not
preserved); their exact byte lengths and SHA-256 values are in the JSON manifest. This is a source audit, not an installed
RVM runtime.

The [official v1.0.0 release](https://github.com/PeterL1n/RobustVideoMatting/releases/tag/v1.0.0)
lists `rvm_mobilenetv3_fp32.torchscript`, asset 44993214, **15,501,891 bytes**.
This author-exported graph is the preferred prospective reference route because
it includes the model/weights and avoids a torchvision port or replacement
architecture. It is not yet acquired or inspected. Its release metadata has no
publisher digest; a future permitted acquisition must record exact bytes and
SHA-256 before loading, identify that digest as locally acquired, and inspect
graph/schema before claiming author-graph fidelity. The separate original
state-dict asset is recorded for provenance, not as a second scored candidate.
No TorchScript/PyTorch or current-runtime parity has been demonstrated.

The existing private ViTMatte environment remains available with Python 3.11.15,
torch 2.8.0, NumPy 2.3.2 and Pillow 11.3.0. Its metadata can inform a temporary
RVM environment, but the author pinned torch 1.9.0/torchvision 0.10.0 in the source
requirements. Reusing torch 2.8.0 is therefore a declared runtime difference
requiring qualification. Record a complete resolved package lock and hashes;
do not install the author's video-decoding dependencies for an image-only adapter
unless actually required. No runtime compatibility is claimed from metadata alone.

The author expressly publishes model inference instructions, which is evidence
of intended use. The README's explicit GPL statement refers to **code**; the
release body is empty and no exact-weight terms were established in this audit.
The [unanswered weight-license question](https://github.com/PeterL1n/RobustVideoMatting/issues/281)
does not supply a license or prove prohibition. Before this work package can
declare a run eligible, retain primary evidence establishing the exact asset's
permission for the intended evaluation. Weight redistribution, commercial
application integration and fixture rights are separate decisions. This plan
does not authorize contacting a publisher or submitting an agreement.

The other concrete prerequisites are a new complete 90-case source-only export
(the existing external exports cover 24 analytic and 42 full cases), an adapter
that imports the model's joint alpha/F without invoking the legacy F estimator,
independent output/quantization/resource checks, runtime qualification and the
final pre-score artifact/package/input freeze. Fixture pins were reverified by
the parallel [evidence audit](phase-9i-feasibility-evidence-audit.md); no fixture reacquisition
was needed. Photographic transmission/F evidence and accepted blinded human
guidance remain absent. These missing inputs cannot be manufactured from the
scoring references.

Unattended work completed here is source/license review, geometry/resource
planning and acquisition-manifest preparation. A future rights-qualified RVM
run could consume the original source RGB unattended; a genuinely assisted
comparison cannot proceed without accepted source-only guidance. This milestone
stops before acquisition/inference and leaves 9I/P7-12, advanced controls and the
original automatic completion gate unchanged.
