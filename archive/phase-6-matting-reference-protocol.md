# Phase 6C photographic references and color qualification

This protocol adds independent foreground-color measurements to the original
24-case alpha gate. It does not qualify an editor feature by itself. The guided
candidate remains disabled; the sampled-alpha and foreground-color candidates
also remain experimental. Passing a cropped portrait benchmark would not show
that general hair, fur, transparent glass and product extraction are ready.

## Reference acquisition and permission

The publisher's [PhotoMatte85 dataset page](https://grail.cs.washington.edu/projects/background-matting-v2/#/datasets)
states: “All datasets are licensed for commercial and non-commercial purposes.”
It also requires a paper citation for academic work and asks commercial-product
users to fill a survey. No survey, account, license agreement or other form was
submitted. This use is local evaluation; photographs are not committed,
redistributed, bundled in the app or used for training. The dataset's permission
is recorded separately from the repository's MIT software license.

The page describes 85 studio portraits donated by a third party, with foreground
and alpha extracted from green-screen photographs. Those supplied annotations
are the reference, not a claim of perfect physical foreground measurements.
The unreleased additional training photographs are excluded.
The paper is [Lin et al., Real-Time High-Resolution Background Matting, CVPR 2021](https://openaccess.thecvf.com/content/CVPR2021/html/Lin_Real-Time_High-Resolution_Background_Matting_CVPR_2021_paper.html).

The executable acquisition recipe is
[scripts/phase6_reference_fixtures.py](../scripts/phase6_reference_fixtures.py).
It reads only the official public archive. It may follow Google's public
file-size virus-scan download confirmation; it refuses login, captcha and terms
forms. It preserves mismatching existing files and interrupted downloads.
Every acquisition/verification checks the exact publisher page asset, archive
and selected members before writing the reference manifest:

| Item | SHA-256 |
| --- | --- |
| [Permission page JavaScript](https://grail.cs.washington.edu/projects/background-matting-v2/static/js/main.7263c829.chunk.js), 42,461 bytes | `58cb834cb99c6ff7d80528f616204039dcb6d1b7d5926c6b05b328feea91121a` |
| [PhotoMatte85.zip](https://drive.google.com/file/d/1KpHKYW986Dax9-ZIM7I-HyBoWVcLPuaQ/view), 413,386,865 bytes | `1d3bd0291c41668238596fa0a5a1f5da51ee213eeba26595e21d54621570ff64` |
| `0051115Q_000001_0003.png` | `9c5f5e0f469ee178d6cdc26ec2274daef2793ef6c8579628e8a9113ff5a9a8d1` |
| `0051115Q_000001_0031.png` | `7e0a553b9358d83a997a771852437ee1a4b24bf0732dbe790f44680a9217f338` |
| `0051115Q_000001_0060.png` | `0cf4107447ad9b13d7c124d6fc8fd9ae5c37cc24d4381af331a71d5209e4dd53` |
| `0051115Q_000001_0088.png` | `6553eac7cea799e82de43c1e054ab84d36ad410dbce4905966c66159431b72d8` |

Four files at archive positions 0, 28, 56 and 84 were selected before candidate
scores. They are 2304 × 3456 RGBA8. Each numerical case uses the same fixed
native-pixel crop `(x:512, y:384, width:1280, height:1280)`, selected before
measurement. Cropping precedes controlled encoded-RGB compositing; there is no
rescaling or alpha inferred from the new composite. Full originals remain in the
private fixture directory, but these results do not qualify full-frame runtime
or accuracy. Each report includes the crop and its zero, opaque and fractional
reference-alpha populations. A crop without fractional coverage cannot provide
positive photographic edge evidence.

The [Adobe Composition-1k access note](https://github.com/MarcoForte/FBA_Matting)
limits Adobe images to noncommercial use, so none were obtained here.
[alphamatting.com](https://alphamatting.com/datasets.php) has useful reference
materials, but no sufficiently clear dataset permission was established for
this use, so none were downloaded. The [withoutBG100 publisher](https://withoutbg.com/tech/datasets/withoutbg100)
currently requires contact for access; nobody was contacted. There is still no
photographic alpha-plus-foreground reference for fur, glass or products in this
qualification set. The existing four licensed inspection photographs remain
unlabelled visual evidence and are not assigned invented numerical truth.

## Frozen alpha and trimap protocol

The original 24 cases remain four analytic subjects, three original backgrounds
and two original starting masks. Their analytic geometry, 4 × 4 area sampling,
encoded compositing, radius-3 blur, binary threshold, metric body and gate are
unchanged. Every case must avoid more than `1/255` boundary-alpha MAD regression
and retain at least 95% of soft details. Refusals are failures. The original
guided runner is rerun to protect its existing 12/24 result during infrastructure
changes; no threshold was changed to accommodate a candidate.

The sampled candidate receives an automatic trimap derived solely from its
initial mask: bytes ≤8 are tentative background, bytes ≥247 tentative foreground,
and each known set is eroded by a clipped 7 × 7 square. Remaining bytes are 128
unknown. This policy was fixed before measurements and reads neither source
colors nor reference alpha. Reference alpha is used only to construct the
predeclared degraded input and score the result. There is no oracle trimap repair.
In particular, a translucent object may have no known foreground sample, and
binary starts may incorrectly pin transparent interiors as background. These
input limitations remain explicit failures, rather than treating a translucent
rim as known opaque foreground.

Photographic references use the same two degraded starts on three controlled
backgrounds: light, dark and checker. They produce 24 additional cases, reported
separately from the unchanged original 24. Their numeric scene is a controlled
recomposition of the supplied extracted foreground and alpha, not the original
green-screen photograph.

## Independent color measurements

All current quality arithmetic uses **encoded RGB**, explicitly matching the
controlled composites. It does not claim linear-light accuracy. Each candidate
foreground call explicitly requests `color_space:"encoded"`; using these metrics
with linear-domain RGB without converting every reference/background would be
invalid. The separate kernel tests cover its declared linear-domain behavior.

The reference foreground is the authored analytic color or supplied photographic
foreground RGB. It is never the contaminated input composite. Measurements are:

- Alpha-weighted foreground RGB MAD, normalized by three times total reference
  alpha weight; invisible colors at reference alpha zero contribute no error.
- Fractional-only foreground RGB MAD, weighted the same way but restricted to
  reference alpha strictly between zero and one. Opaque interiors cannot hide
  visible edge contamination in this score.
- White, dark and checker replacement-background recomposition MAD and boundary
  MAD. These compare `estimated_alpha × estimated_F + (1−estimated_alpha) × B`
  with the independently known reference composite over each replacement `B`.
- Changed source RGB bytes at reference-opaque pixels, and mean false-positive
  alpha on reference background pixels.

The additional color gate was frozen before candidate measurements: preserve
opaque source RGB exactly; do not regress alpha-weighted foreground MAD by more
than `1/255`; fractional foreground MAD must be ≤0.05; where the initial
fractional error exceeds 0.02, reduce it by at least 20%; replacement-background
boundary MAD must be ≤0.02 on all three backgrounds. An unchanged visibly
contaminated foreground therefore does not qualify merely by avoiding regression.
A reference without fractional pixels is diagnostic rather than a color pass.

This is **gate v1**. It remains the rule the Rust evaluators record as
`passes_end_to_end_gate`, and it is still reproducible with `--gate v1`. For 9I
completion it is superseded by [gate v2](phase-9i-gate-v2-status.md), which keeps
every threshold above and changes only two things: the analytic pass rule keeps
the twelve recoverable cases (ambiguous cases become reported diagnostics and
glass moves to the transmission-research track), and "preserve opaque source RGB
exactly" becomes a bounded rule (≤2 code levels, ≤1% of reference-opaque pixels,
and every changed pixel inside the boundary set).

Reports keep three conclusions separate: **oracle-alpha color** supplies exact
reference alpha to isolate foreground estimation; **sampled-alpha color** uses the
sampled candidate's alpha to measure the coupled color result; **end-to-end**
requires both the alpha and sampled-alpha color gates. A refused alpha candidate
cannot pass end-to-end; unchanged prior metrics are retained only as diagnostics.
Oracle-alpha color success cannot substitute for alpha qualification.

Artifacts include full native-size source, prior, trimap, reference alpha,
estimated alpha, confidence and estimated foreground. For each analytic case and
the fixed light/blurred case of each photo, three backgrounds show reference,
prior with observed RGB, sampled alpha with observed RGB, and sampled alpha with
estimated foreground RGB. Four-column panels use a fixed top-left 512 × 512
native crop; the corresponding full-size composites allow inspection elsewhere.

## Reproduction and result semantics

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase6_reference_fixtures.py ~/devel/rasterize-fixtures/phase6/references
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_phase6_reference_fixtures.py
cd core
CARGO_BUILD_JOBS=4 cargo test --release --test matting_qualification
CARGO_BUILD_JOBS=4 cargo build --release --example phase6_selection_quality --example phase6_matting_quality
cd ..
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase6_matting_qualification.py \
  ~/devel/rasterize-fixtures/phase6/references \
  ~/devel/rasterize-fixtures/phase9i-reports/rz-matting-run --gate v2 --require-pass
PYTHONDONTWRITEBYTECODE=1 python3 scripts/phase6_matting_diagnostics.py \
  ~/devel/rasterize-fixtures/phase9i-reports/rz-matting-run --opaque-artifacts
```

The output directory must be new and outside the repository. The wrapper verifies
all source pins before running, records executable SHA-256 values, preserves
stdout/stderr logs, reruns the original guided baseline and writes `run.json`
plus detailed `matting/metrics.json` and `failure-reasons.json`. Successful process exit means measurements
completed; `--require-pass` additionally returns failure unless every analytic and
photographic case passes end-to-end. Neither mode replaces photographic visual
inspection or supplies missing subject-class references.

Independent small tests verify a hand-computed fringe/recomposition error,
zero color error with wrong alpha, opaque-byte changes, invalid dimensions and
nonfinite RGB. Acquisition checks cover changed checksums/storage/permission,
repository output refusal, refusing legal forms and preserving a pre-existing
partial download. Candidate accuracy is not encoded as a test expectation.

## Recorded first run: September 9, 2026

The strict wrapper completed in 67.2 seconds and returned exit 1 for the measured
quality failure. There was no runner error and no threshold or candidate tuning
after measurement. The original guided baseline remained 12/24.

| Independent conclusion | Original analytic cases | Controlled photographic crop cases |
| --- | ---: | ---: |
| Sampled alpha gate | 10/24 | 8/24 |
| Alpha refusal | 6/24 | 0/24 |
| Foreground color given reference alpha | 20/24 | 22/24 |
| Color given sampled alpha | 0/24 | 0/24 |
| Combined alpha and color gate | 0/24 | 0/24 |

Oracle-alpha rows intentionally retain the two original starting-mask labels for
alignment with the alpha report; the oracle calls for a given source are duplicates.
Thus their counts represent 10/12 distinct analytic scenes and 11/12 distinct
photographic scenes, rather than 24 independent foreground-color experiments.

All six analytic glass cases lacked a valid known foreground sample and were
refused. Each photo crop contains genuine reference background, opaque foreground
and fractional coverage: fractional populations are respectively 99,693, 218,960,
34,244 and 29,852 pixels. No empty soft population produced a photographic pass.

Every non-refused sampled-alpha case changed some RGB at **reference-opaque**
pixels, failing the strict preservation gate. This is not a violation of the
foreground kernel's narrower contract to preserve RGB where its **supplied**
alpha equals 255: inaccurate estimated alpha can mark a true opaque pixel as
partial. Nor is byte preservation the only issue: replacement-background boundary
MAD exceeds 0.02 in 12 analytic and 19 photographic cases. The two photographic
oracle-color failures are the same first-portrait checker scene under both start
labels, with fractional foreground MAD 0.06168 against the 0.05 ceiling.

Failure criteria overlap, so their counts must not be summed as a count of failed
cases. `failure-reasons.json` records each criterion independently, and the
optional artifact audit measures opaque RGB byte deltas from existing PNGs
without rerunning candidate kernels. Across all 24 exported analytic cases, the
largest changed reference-opaque RGB channel differs by 11 byte levels. In the
four exported photo light/blurred cases the maxima are 54, 59, 47 and 49, affecting
3,928, 4,698, 5,742 and 6,331 pixels respectively. The other 20 photographic cases
have no exported foreground image and their byte deltas are explicitly
unmeasured. Independently decoded PNG changed-pixel counts match the metric
report. This separates useful foreground estimation with trustworthy alpha from
the unresolved automatic alpha and coupled output.

The private run is `/tmp/rz-phase6c-qualified` (purged from /private/tmp; not
preserved): `run.json` pins the measured
executables, `matting/metrics.json` contains every numeric result and reference
record, and `matting/` contains full native-size comparisons. The numerical
failure keeps both candidates experimental and does not qualify advanced
refinement or color decontamination for editor exposure.
