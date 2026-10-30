# Phase 6C trimap sampling candidate

This is an experimental Rust kernel, with no editor or MCP exposure. It does not
replace the failed guided candidate or weaken its existing 24-case quality gate.
The original corpus, degradations, boundary-error allowance and soft-detail recall
threshold remain unchanged. Refused cases count as failures.

## Model and evidence

For an observed RGB color `C`, known foreground color `F` and background color `B`,
the candidate solves `C = alpha F + (1-alpha) B` by projecting `C-B` onto `F-B`.
It ranks candidate pairs by reconstruction residual and normalized spatial
distance. These are equations 2–5 in
[He et al., A Global Sampling Method for Alpha Matting, CVPR 2011](https://www.microsoft.com/en-us/research/wp-content/uploads/2011/06/HeEtAl.pdf).
The implementation uses a bounded search described below, not the paper's
SampleMatch search or its reported accuracy.

Unreliable sample estimates should remain visible to callers; this follows the
distinction between sample fitting and sample confidence in
[Wang and Cohen, Optimized Color Sampling for Robust Matting, CVPR 2007](https://grail.cs.washington.edu/wp-content/uploads/2015/08/wang2007robust.pdf).
Our confidence is a separation/residual diagnostic, not a calibrated probability
and not that paper's random-walk optimization.

The scalar-alpha equation is insufficient for general refractive glass or
channel-dependent transmission. A different background can change the observed
glass through refraction, which cannot be represented by one fixed foreground
RGB and scalar alpha. See
[Yeung et al., Matting and Compositing of Transparent and Refractive Objects](https://www.microsoft.com/en-us/research/wp-content/uploads/2011/01/armLowRes.pdf).
The existing analytic glass is a scalar-alpha diagnostic; passing it would not
qualify physical glass extraction.

## Contract

`sampled_matting::estimate` accepts finite RGB triples in `0..=1`, explicit
dimensions, a categorical trimap, a prior coverage plane, sampling parameters,
and an optional single-use job control. The caller selects the compositing
domain. Existing analytic fixtures use encoded RGB because their composition is
encoded; linear-light work requires decoding with the document profile before
this call. There is no implicit sRGB conversion in this kernel.

Trimap bytes are exactly `0` background, `128` unknown and `255` foreground.
Known labels are hard constraints. Relabeling a pixel unknown permits its alpha
to change; a wrong known label cannot be repaired by this solver. Inputs are
immutable. A complete result contains alpha, foreground/background sample colors
in the supplied domain, confidence, and sample/unresolved counts. Sample colors
are meaningful for solved unknown pixels. At known pixels the observed color is
preserved for its known class, while the unobserved class is zero and must not be
used as recovered color evidence.

Unknown pixels with no sufficiently separated, adequately fitting pair retain
their prior byte and have zero confidence. Missing foreground or background
boundary samples cause an explicit refusal. Refusal is important for wholly
translucent objects: a partial rim cannot automatically become a trusted opaque
foreground sample.

## Bounded algorithm

Known pixels immediately adjacent to unknown pixels form the two sample sets.
A median-split spatial tree supplies the exact eight nearest samples per class.
Up to eight further colors per class are selected by deterministic farthest-color
sampling over the global boundary. Each unknown pixel therefore tests at most
256 pairs. This is not a second implementation of the core's PatchMatch engine.

Defaults were fixed before corpus measurement: minimum RGB separation `12/255`,
maximum reconstruction residual `8/255`, spatial weight `1/255`. The residual
uses projected alpha clamped to `0..1`; scores use residual plus distance from
each sample divided by the nearest corresponding boundary distance. Low color
contrast is treated as uncertainty rather than amplified into a confident alpha.
No per-subject thresholds or ground-truth comparisons occur in the kernel.

The operation refuses more than 1,000,000 unknown pixels or 250,000 total
boundary samples. These are explicit work limits, not a silent preview scale.
The conservative reservation is `26 × canvas_pixels + 5,000,000 + 65,536` bytes,
excluding borrowed RGB/trimap/prior. Output alpha and confidence use two bytes
per pixel; two float RGB estimates use 24. Boundary vectors become spatial
indices in place; no second full tree allocation or sort scratch is retained.
Tree and palette construction poll cancellation, and sample search polls every
64 pixels and each completed row. Spatial searches also poll every 4,096 visited
nodes: a degenerate boundary is not assumed to give logarithmic search cost.
Full-canvas output still makes large-image
work expensive even when a brush marks a small unknown region; regional output
would be a separate optimization with its own contract.

## Verification and qualification separation

Focused independent tests cover exact compositing projections (including partial
alpha 0.03, 0.16, 0.34, 0.7 and 0.97), pinned constraints, missing foreground
evidence, indistinguishable colors, an inconsistent color line, distant color
evidence, nearest-boundary ranking, malformed data, resource refusal and live
cancellation. The partial-alpha test supplies known foreground/background
endpoints separately. It demonstrates identifiability with additional evidence;
it is not an automatic glass result or an original quality-gate pass.

The focused optimized suite passed all ten tests in 0.17 seconds. Cancellation
after sample search began returned in 0.042 ms; the separate 5 × 40,000 collinear
boundary case returned in 0.044 ms. These are observed test timings, not a general
latency guarantee. The regression requirement is less than 500 ms, with no result
publication and zero retained job scratch. Evidence is in
`/tmp/rz-phase6-sampled-matting-tests.log`.

The qualification runner derives its automatic trimap solely from the degraded
prior: values at most 8 are background, values at least 247 foreground, and both
known sets are eroded by three pixels. The remaining pixels are unknown. This
policy is frozen for comparison, not asserted to produce correct constraints.
In particular, a binary glass prior can mislabel transparent interior as known
background; a useful workflow needs explicit unknown corrections and additional
color evidence. No alpha ground truth repairs the automatic trimap.

## Measured failure and next requirements

The numerical qualification failed. The sampling candidate passed the alpha gate
on 10 of 24 original analytic cases, with all six automatic glass cases refused,
and on 8 of 24 controlled photographic recompositions. No case passed the joint
alpha-plus-color gate in either set. Foreground estimation with separately
supplied oracle alpha passed its color gate on 20 of 24 analytic and 22 of 24
photographic cases; those counts are component diagnostics, not complete cutout
successes. The photographic set comprises fixed 1280 × 1280 portrait/hair crops
with supplied extracted foreground/alpha references, not physical ground truth
or photographic coverage of fur, glass and products.

Minimum sample separation does not identify every ambiguous image. For
`hard_product-ambiguous-binary`, boundary MAD increased from 0.001745 to 0.202353
and soft-detail recall fell to 0.85, despite zero unresolved pixels. Spatial color
variation supplies widely separated global samples along nearly the same color
line; several alpha/sample pairs can explain the observation. The current
confidence therefore must not be treated as reliable authority to change pixels.
`portrait_hair-ambiguous-blurred` worsened from 0.141659 to 0.334043 boundary MAD.
The worst photographic regression was a checker-background portrait crop, from
0.012739 to 0.059840. Successful sample-search calls took 15–138 ms on the analytic
cases and 138–1,991 ms on the photographic crops in this run.

These results require a stronger ambiguity model and more useful trimap/color
evidence before another candidate can qualify. Estimated alpha also becomes
fractional at some reference-opaque pixels; the subsequent color stage then
edits them and fails the opaque-preservation check. This is a coupled pipeline
failure even when color estimation works with correct alpha. Future work must
retain the original cases, add the competing-pair counterexample, and rerun both
alpha and color gates. Whole-image photographic inspection and appropriate
fur/glass/product references remain separate requirements. Nothing is exposed
in the app based on these failed results.

Evidence: `/tmp/rz-phase6c-qualified/matting/metrics.json`,
`/tmp/rz-phase6c-qualification.log`, and
`/tmp/rz-phase6c-quality-diagnostics.log`. An independent rerun of the original
guided candidate preserved all 24 before/after metric sets and pass flags
exactly, retaining its original 12/24 result; the parity record is
`/tmp/rz-phase6c-guided-baseline/parity.json`.
