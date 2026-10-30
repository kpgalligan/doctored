# Closed-form (color-line) alpha candidate (ADR)

- **Status:** Accepted — open items remain (candidate kept experimental; the cutout gate it was measured against is open)
- **Phase / dates:** 6C, 2026-09-10
- **Original:** [archive/phase-6-closed-form-matting-candidate.md](../archive/phase-6-closed-form-matting-candidate.md) — full history, measurements, receipts
- **Key commits:** 0155d41

## Context
The [sampled candidate](phase-6-sampled-matting-candidate.md) picks foreground/background pairs independently per
unknown pixel and fails badly on ambiguous edges. This experiment adds a spatial alpha solver — the RGB matting
Laplacian of Levin, Lischinski and Weiss (*A Closed-Form Solution to Natural Image Matting*, eqs. 10–12) — without
touching the sampled/guided candidates, their defaults, or any frozen gate. No editor control or MCP tool exposes it.

## Decisions
- **Model:** local affine RGB→alpha fit over all fully contained 3×3 windows (smaller dimensions use their actual
  extent); per-window matrix `I − 11ᵀ/m − X(XᵀX + εI)⁻¹Xᵀ`. Windows with no unknown pixels are omitted exactly.
  *Why:* a spatially coupled objective instead of independent per-pixel sample pairs.
- **Constraints:** trimap `0`/`255` are exact Dirichlet constraints moved to the RHS; only `128` pixels are solved. The
  prior is only the initial iterate — no hidden prior penalty, no reference alpha. *Why:* keeps the solver honest; a
  wrongly pinned label cannot be repaired by the solver.
- **Entry point:** `closed_form_matting::estimate` (`../core/src/closed_form_matting.rs`) takes immutable normalized
  RGB, dimensions, categorical trimap, prior coverage, `ClosedFormParams`, optional `DrJobControl`. RGB must be finite in
  `0..=1` in a caller-selected compositing domain; **no implicit sRGB curve**. Callers using profile-linear RGB must pick
  a matching ε. (9I later added `estimate_soft_prior` with `user_constraints` beside it.)
- **Defaults:** ε = `9/(12·255²)` ≈ 1.1534e-5 (summed variance of nine uniform 8-bit quantization errors — a
  measurement-scale rationale, not per-subject tuning); max 500 iterations; true residual RMS ≤ 1e-6 over unknowns.
- **Numerics:** specialized 3×3 Cholesky for the regularized covariance (the ICC matrix inverse rejects determinants
  that are normal for near-rank-one covariances, so it is deliberately not reused); symmetric 25-neighbour stencil
  instead of a dense matrix; PCG with symmetric Gauss–Seidel preconditioner `M = (D+L)D⁻¹(D+L)ᵀ`. Convergence is
  confirmed by recomputing `b − Ax`; an optimistic recurrence residual restarts.
- **Errors:** nonconvergence returns an error with iteration count, RMS and relative residual — never a partial matte.
  Missing FG/BG constraints, malformed input, budget overrun and solver failure leave inputs unchanged and release
  scratch. Success reports residual, iterations, window count and count of unknowns clipped to `0..1`.
  Numerical convergence is **not** an identifiability/confidence score; no confidence plane is produced.
- **Bounds (refuse, never shrink):** `MAX_UNKNOWN_PIXELS = 500_000`, `MAX_ACTIVE_WINDOWS = 1_000_000`,
  `MAX_STENCIL_TERMS = 20_000_000_000` (five 25-term passes per unknown per allowed iteration plus setup; can refuse
  above ~319k unknowns at default iterations). Scratch reservation `5N + 260U + 65,536` bytes (N canvas, U unknown),
  excluding borrowed inputs. All phases poll cancellation; output copies use the shared checked 4,096-byte helper.
- **Qualification harness:** `../core/examples/phase6_closed_form_quality.rs` reuses the frozen evaluator; the
  candidate callback sees only observed RGB, trimap and degraded prior. Refusals and nonconvergence count as failures.

## Rejected alternatives
- Reusing the ICC 3×3 inverse — its determinant threshold rejects ordinary matting covariances.
- Jacobi preconditioning — two photographic prototype solves hit the iteration bound; SGS changes speed, not objective.
- ε = 1e-7 (prototype) — fur solves failed to converge; replaced by the quantization-scale rationale.

## Consequences
- Result: alpha 10/24 analytic, 9/24 photo crops, 15/42 full images; **0 joint alpha/color passes** in every set.
  Boundary MAD improved on 28/42 full images but regressed elsewhere (e.g. `0031` checker/blurred, ambiguous
  hard-product binary 0.202 → 0.290). Equal pass counts with sampling do not mean equal output.
- The constraint audit shows 6/24 analytic and 6/24 crop inputs cannot pass under their frozen known labels regardless
  of solver: better constraints or user evidence are required, not a better solver alone.
- The same objective, ε and 20B stencil budget are reused by `../core/src/tiled_soft_matting.rs`, the kernel behind the
  9I assisted protocol (`assisted_tiled_soft_prior_v1`); changing these constants changes that declared kernel.
- Scalar alpha + fixed foreground cannot represent refractive glass; glass cases without foreground constraints refuse.

## Open items (verified 2026-09-26)
- `TRACKED` — Coupled alpha/color qualification, stronger input evidence and full-image photographic inspection →
  carryover P7-12 / plan 9I, gate v2 assisted protocol ([phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md)).
- `DROPPED` — Glass/transmission recovery: glass left the pass rule for a reported-only glass diagnostic (transmission research declined) (gate v2
  decision 1, [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
