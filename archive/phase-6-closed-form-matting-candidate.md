# Phase 6C local color-line alpha candidate

This experiment adds a spatial alpha solver without changing the sampled or
guided candidates, their defaults, the original 24-case gate, or the separate
foreground/recomposition gates. No editor control or MCP tool exposes it.

## Model

The model is the RGB matting Laplacian from Levin, Lischinski and Weiss,
[A Closed-Form Solution to Natural Image Matting](https://doi.org/10.1109/TPAMI.2007.1177),
equations 10–12. A local affine relationship between RGB and alpha is fitted in
overlapping windows. Eliminating its color coefficients gives a quadratic alpha
objective. The paper also explains the regularizer's tradeoff: very small values
preserve image noise, while larger values favor smoother alpha. This is a local
color-line assumption, not proof that foreground and background are identifiable.

For a window of `m` pixels, let `X` contain mean-centered RGB values. Its local
matrix is `I - 11ᵀ/m - X(XᵀX + epsilon I)⁻¹Xᵀ`. Summing overlapping windows
produces the image operator. All fully contained 3 × 3 windows participate; a
dimension smaller than three uses its actual extent. Windows containing no
unknown pixels can be omitted from the reduced system exactly.

Categorical trimap values `0` and `255` are exact Dirichlet constraints. Their
matrix columns move into the right-hand side; only `128` pixels are solved.
The prior supplies the initial iterate, with no hidden prior penalty or access
to reference alpha. This avoids selecting unrelated foreground/background
sample pairs independently at each unknown pixel, but it cannot recover detail
that the supplied trimap incorrectly pins as background.

## Numerics and contract

`closed_form_matting::estimate` accepts immutable normalized RGB triples,
dimensions, categorical trimap, prior coverage, `ClosedFormParams`, and optional
`RzJobControl`. RGB is finite and within `0..=1` in an explicitly selected
compositing domain. The kernel does not silently apply sRGB curves. Existing
encoded-RGB fixtures remain encoded; a caller using profile-linear RGB must
choose the corresponding regularization scale.

The default epsilon is `9/(12 × 255²)`, approximately `1.1534e-5`: the summed
variance of nine independent uniform 8-bit quantization errors. This is a
measurement-scale rationale, not a per-subject setting. Other defaults are a
maximum 500 iterations and true residual RMS at most `1e-6` over unknown pixels.

Construction uses a specialized 3 × 3 Cholesky solve for the regularized
covariance. The existing ICC matrix inverse deliberately rejects determinants
that are ordinary for these nearly rank-one covariance matrices, so its
profile-matrix threshold is not reused. Each window's contributions are inserted
symmetrically into a 25-neighbor stencil. This preserves the local operator
while avoiding an image-sized dense matrix and repeated covariance work during
iteration.

Preconditioned conjugate gradients uses symmetric Gauss-Seidel,
`M = (D + L)D⁻¹(D + L)ᵀ`, where `L` is its strictly lower-triangular part.
A positive diagonal makes this preconditioner positive
definite even when the matting operator has positive off-diagonal entries. It
changes convergence speed without changing the objective. Before accepting
convergence, the solver recomputes `b - Ax`; an optimistic recurrence residual
causes a restart. Nonconvergence returns an error including the iteration count,
RMS residual and relative residual. It does not publish a partial matte.

Success reports the true residual before output clipping, iteration and window
counts, and the number of unknown values clipped to `0..1` before byte encoding.
Known labels remain exact. Numerical convergence is explicitly not an
identifiability-confidence score. No confidence plane is produced.

## Bounded execution

- At most 500,000 unknown pixels and 1,000,000 active windows. Limits refuse;
  they do not silently shrink the image or change the trimap.
- A separate cap permits at most 20 billion estimated stencil terms, counting
  five 25-term passes per unknown pixel per allowed iteration plus setup. With
  default iterations this can refuse above roughly 319,000 unknown pixels,
  before the independent 500,000-pixel ceiling is reached.
- Reserved scratch, including output and excluding borrowed inputs, is
  `5 × canvas_pixels + 260 × unknown_pixels + 65,536` bytes. The full-canvas
  index and output use `5N`; stencil, RHS, unknown positions and five numerical
  vectors use at most `252U`, with alignment headroom in the reservation.
- Input/window preparation, stencil validation, matrix products,
  preconditioning, reductions and output copies poll cancellation. Both known-only
  and solved coverage use the same checked 4,096-byte output-copy helper.
- Missing foreground/background constraints, malformed input, exceeded budgets
  and solver failure leave all caller inputs unchanged and release job scratch.

## Development measurements

The independent Python prototype used a directly assembled SciPy sparse matrix,
not the Rust stencil/Cholesky implementation. Its first configuration,
epsilon `1e-7` with a Jacobi preconditioner, passed 8 of 24 analytic cases; two
fur solves exceeded 500 iterations and all six glass cases lacked foreground
constraints. The quantization-scale epsilon converged on all 18 non-glass cases
and passed 10 of 24. These are retained development outcomes, not a change to
the frozen gates.

All four previously saved photographic light/blurred artifact cases were also
tested. Two Jacobi solves converged and reduced boundary MAD relative to the
sampled candidate from `0.016590` to `0.010824` and from `0.018208` to `0.010167`.
Two reached the iteration bound and were counted as refusals. This motivated
the stronger numerical preconditioner, without changing the alpha objective or
selectively dropping difficult cases. Python evidence is under
`/tmp/rz-closed-form-prototype/`,
`/tmp/rz-closed-form-quantization-prototype/` and
`/tmp/rz-closed-form-photo-prototype/`. Rust qualification is recorded separately.

The focused Rust tests compare against independently assembled, uncentered
four-variable normal equations solved with pivoted dense elimination. They
cover ordinary and 1 × N / 2 × N geometry, soft alpha, exact constraints,
missing foreground evidence, malformed data, nonconvergence, resource refusal
and live iterative cancellation. The shared qualification example accepts
only observed RGB, trimap and degraded prior through its candidate callback;
reference alpha and foreground do not enter the solver.

The first optimized focused run passed all seven tests in 0.05 seconds; all
tests and examples also passed Cargo check. Evidence is
`/tmp/rz-phase6c-closed-form-tests.log`. These correctness tests are separate from
numerical image-quality qualification.

## Rust qualification results

The final stencil/SGS implementation still fails the complete quality gate.
Both numerical runs finished successfully as measurements, with failed quality
results retained rather than reclassified as release passes.

| Set | Sampled alpha passes | Closed-form alpha passes | Closed-form joint alpha/color passes |
| --- | ---: | ---: | ---: |
| Original analytic cases | 10/24 | 10/24 | 0/24 |
| Existing photographic crop recompositions | 8/24 | 9/24 | 0/24 |
| Full native-image recompositions | 15/42 | 15/42 | 0/42 |

The analytic run retains six missing-foreground glass refusals. All other cases
converged numerically; the cropped and full-image runs had no solver/admission
refusals. The full-image set contains seven source images over three backgrounds
and two degraded priors, not 42 independent photographs. Its bottle reference
tests an extracted silhouette and edge alpha, not physical translucent glass.

Compared with sampling, boundary MAD improved on 6 of 18 solved analytic cases,
12 of 24 photographic crops and 28 of 42 full-image cases. This is mixed progress,
not an across-the-board replacement. The full native longhair-fur light/blurred
case and portrait `0060` dark/binary case newly pass the alpha gate; two bottle
silhouette cases lose their prior pass. The largest photographic crop gain is
`0060` dark/blurred, from `0.059752` to `0.031669` boundary MAD. A counterexample is
`0031` checker/blurred, which regresses from `0.057406` to `0.074434`. Ambiguous
hard-product edges remain poor: binary boundary MAD increases from `0.202353`
under sampling to `0.289946` under closed form.

All final solves used at most 279 iterations. Measured kernel times were
8–46 ms for solved analytic cases, 86–3,083 ms for photographic crops, and
63–4,299 ms for full images (full-image median 306 ms). Qualification overlapped
other verification work, so these are observed timings, not isolated performance
guarantees. The 48-case analytic/crop run took 44.4 seconds overall and the
42-case full-image run took 178.6 seconds including color estimation and artifacts.

The post-hoc constraint audit proves that six analytic inputs and six crop
inputs cannot satisfy the alpha gate while preserving their known labels. For
example, binary portrait crops pin enough true detail to background that their
maximum possible recall is only `0.6473` or `0.8181`; the gate requires `0.95`.
Reference alpha is used only to explain this bound, never to repair the trimap
or guide the solve. The constraints need correction or additional user evidence.

Native-pixel inspection of two dark-background portrait panels and a longhair
fur panel confirms reduced bright fringes in some results. The difficult `0031`
hair edge still widens or smears after color estimation. No visual or numerical
result here qualifies the coupled output; the strict color gate remains failed
on every case, including opaque-pixel preservation failures.

Reports and native-size artifacts are `/tmp/rz-closed-form-quality/metrics.json`
and `/tmp/rz-closed-form-full/metrics.json`, with run logs beside those directories.
The comparison sampled full-image report is
`/tmp/rz-phase6-full-quality/images/metrics.json`. The separate sampled parity
check passed on all 48 existing cases and all report fields except the newly
added constraint audit and runtimes; evidence is
`/tmp/rz-phase6c-sampled-parity/exact-parity.json`. The guided baseline remains
12/24. No baseline metric or quality threshold was changed.

## Remaining requirements

Numerical improvements on selected color-line scenes do not complete Phase 6C.
The unchanged alpha/color qualification remains failed. Further full-image
photographic inspection and stronger input evidence remain necessary. The automatic glass cases lack opaque
foreground constraints and remain refusals. General refractive glass additionally
exceeds a fixed foreground plus scalar-alpha compositing model. Ambiguous scenes
may have multiple physical explanations even when the quadratic system has a
unique numerical minimizer; more suitable user constraints or captured color
evidence remain necessary.

The new runner reuses the frozen evaluator:

```sh
cargo run --release --example phase6_closed_form_quality -- \
  /tmp/rz-closed-form-quality /tmp/rz-photo-matte-references
cargo run --release --example phase6_closed_form_quality -- \
  --full-images /tmp/rz-closed-form-full /tmp/rz-full-matting-references
```

Nonconvergence and admission refusals count as failures, with prior coverage
retained only for diagnostic comparisons. Reference-alpha color results remain
separate from estimated-alpha and joint pipeline results.
