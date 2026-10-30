# Phase 6C foreground evidence and ambiguity

The existing foreground estimator is useful when supplied alpha is accurate,
but it does not establish which pixels are truly opaque. This continuation adds
an ambiguity regression and a reproducible numerical diagnostic. It does not
add a foreground correction option, change the original quality gates, or expose
advanced refinement controls.

## Why a color-only correction cannot certify opacity

For an observed color `C`, the compositing equation is
`C = alpha F + (1-alpha) B`. The current multilevel estimator treats the supplied
alpha as input and estimates spatially regularized `F` and `B`. That is the
problem defined by
[Germer et al., Fast Multi-Level Foreground Estimation](https://arxiv.org/html/2006.14970v1).
A successful color solve does not validate the supplied alpha.

Consider an unknown pixel between correctly marked black background and white
foreground. The observed pixel is gray, `C = 128/255`. Two scenes produce exactly
the same RGB image and obey the same known trimap labels:

- An opaque gray mark: `alpha = 1`, `F = C`.
- Translucent white over black: `alpha = 128/255`, `F = 1`, `B = 0`.

The surrounding source, supplied prior, and categorical trimap can also be
identical. Perfect reconstruction and strong foreground/background separation
therefore do not choose between the scenes. Preserving RGB in the opaque scene
requires `F = C` exactly; recovering foreground in the translucent scene requires
a value nearly half the normalized gamut away. A deterministic operation with
those same inputs cannot satisfy both requirements without additional evidence
or a more restrictive scene assumption.

`core/tests/foreground_ambiguity.rs` constructs those two scenes with identical
inputs and valid known constraints. The existing sampling candidate reports
maximum fit confidence at the ambiguous pixel. The existing color FFI improves
the translucent scene yet changes the reference-opaque pixel in the other
scene. The strict opaque-preservation gate catches that failure. A further
counterexample demonstrates that projecting an estimated `(F,B)` pair to exact
compositing consistency can increase foreground error even when alpha is
correct. These are evidence limitations, not permission to relax the gate.

The sampling literature distinguishes color fit from confidence and adds spatial
priors; it does not provide certainty from a small residual alone. See
[Wang and Cohen, Optimized Color Sampling for Robust Matting](https://grail.cs.washington.edu/wp-content/uploads/2015/08/wang2007robust.pdf),
and the ambiguity limitation in
[He et al., A Global Sampling Method for Alpha Matting](https://www.microsoft.com/en-us/research/wp-content/uploads/2011/06/HeEtAl.pdf).

## Feasibility experiment

A limited correction can be derived without fitting thresholds: constrain each
foreground channel to values that admit *some* background channel in `[0,1]`
under the supplied alpha. In encoded byte units, with observed byte `C` and alpha
byte `A > 0`, the allowed integer foreground interval is

```
lower = max(0, ceil(255 * (C - 0.5 - (255 - A)) / A))
upper = min(255, floor(255 * (C + 0.5) / A))
```

Half a source byte accounts for quantization. At `A = 0`, all foreground values
are unconstrained. At `A = 255`, the only integer foreground is the observed
source byte. The experiment clamps the existing estimated foreground into this
interval. Its inputs are only source RGB, estimated alpha, and estimated
foreground; reference alpha is used afterward to count damage. The projection
is a diagnostic hypothesis, not a new core operation.

The interval is independently tested by enumerating all 256 foreground byte
values and checking whether varying background across its gamut can intersect
the observed source's quantization interval. The two diagnostic tests also
check opaque identity and the fact that `F = B = C` is always feasible.

On the frozen previous candidate outputs, the projection did not resolve the
coupled failure:

| Artifact set | Measured cases | Changed reference-opaque pixels before | After feasibility projection | Cases still changing opaque RGB |
| --- | ---: | ---: | ---: | ---: |
| Original analytic cases | 24 | 11,502 | 11,502 | 18 |
| Saved photographic light/blurred cases | 4 | 20,699 | 20,669 | 4 |

The other 20 photographic cases have no saved foreground artifacts and are
explicitly unmeasured in this experiment. The four measured crops retained
maximum opaque-channel changes of 47–59 bytes after projection. The analytic
ambiguous cases also include truly opaque pixels whose estimated alpha is below
128; protecting only alpha values close to 255 would leave those failures.

The projection's feasible color range is too broad to identify the correct
foreground. It is therefore not integrated as a correction option. These counts
are diagnostics of unchanged candidate artifacts, not a new full quality run.

Reproduce after the existing qualification artifacts have been generated:

```sh
python3 -B scripts/test_phase6_foreground_evidence.py
# /tmp/rz-phase6c-qualified was purged from /private/tmp and is not preserved;
# point this at a new run under ~/devel/rasterize-fixtures/phase9i-reports/.
python3 -B scripts/phase6_foreground_evidence.py \
  ~/devel/rasterize-fixtures/phase9i-reports/<run> \
  --include-photographs \
  --output ~/devel/rasterize-fixtures/phase9i-reports/foreground-evidence.json
cargo test --release --manifest-path core/Cargo.toml --test foreground_ambiguity
```

The JSON identifies the input metrics by SHA-256, records per-case estimated
alpha bins, original and projected opaque damage, and missing artifact cases.
It never overwrites the original metrics, images, or gates.

## Remaining requirements

Reliable constraints, an explicit correction footprint, or stronger learned
scene priors may improve practical behavior. A user's Unknown brush authorizes
estimation; it does not prove that every covered pixel is translucent. Any
correction must still pass the reference-opaque gate, the separate alpha and
foreground metrics, and replacement-background inspection. No truth-derived
mask may be supplied to a candidate to manufacture that result.

Two concrete research directions have author-maintained implementations:
[ViTMatte](https://github.com/hustvl/ViTMatte) has an MIT-licensed repository,
pretrained checkpoints, and image-plus-trimap inference; it supplies a stronger
alpha prior rather than proof of foreground-color correctness.
[BackgroundMattingV2](https://github.com/PeterL1n/BackgroundMattingV2) is also
MIT-licensed and explicitly requires an additional captured background image.
That changes the evidence and workflow. Neither has been integrated or
qualified here; checkpoint terms, deployment cost, the unchanged corpus, and
joint alpha/color behavior would need evaluation before adoption.
