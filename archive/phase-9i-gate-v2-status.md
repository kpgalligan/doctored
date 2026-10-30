# Phase 9I — cutout gate v2

**9I, P7-12 and Phase 9 remain open.** Gate v2 was frozen on 2026-09-15 and is
recorded here verbatim before any re-scoring ran. No app code, FFI signature,
Swift file or experimental control changed in this milestone. One test-support
metric was added (`color_metrics` in `core/tests/common/matting_quality.rs` now
emits two extra numbers), no candidate was re-run, and no candidate passes gate
v2. The five preserved candidate reports are re-scored under both gates below;
the best automatic result on the twelve analytic pass-rule cases is 3 of 12, and
no candidate passes the 24 photographic crops or the 42 full images.

This document records six owner decisions delegated by the repository owner on
2026-09-15. They are recorded, not reopened. It supersedes specific sentences in
[the matting reference protocol](phase-6-matting-reference-protocol.md), [the
feasibility decision](phase-9i-feasibility-status.md) and [the established
pipeline decision](phase-9i-established-pipeline-decision.md); every historical
number in those documents stands.

## Gate v2, frozen verbatim

Gate v2 keeps every threshold of gate v1 (the "Independent color measurements"
section of [the matting reference protocol](phase-6-matting-reference-protocol.md)).
It changes the case scope of the analytic pass rule and one clause of the color
gate. Nothing else moves.

**1. Analytic pass rule — 12 cases.** The pass rule is exactly
`{portrait_hair, fur, hard_product}-{light, dark}-{blurred, binary}`:

```
portrait_hair-light-blurred   portrait_hair-light-binary
portrait_hair-dark-blurred    portrait_hair-dark-binary
fur-light-blurred             fur-light-binary
fur-dark-blurred              fur-dark-binary
hard_product-light-blurred    hard_product-light-binary
hard_product-dark-blurred     hard_product-dark-binary
```

**2. Ambiguous diagnostic — 6 cases, reported, not gating.**
`{portrait_hair, fur, hard_product}-ambiguous-{blurred, binary}`.

**3. Glass transmission-research track — 6 cases, reported, not gating.**
`translucent_glass-{light, dark, ambiguous}-{blurred, binary}`. Glass takes
precedence over the ambiguous rule, so the two ambiguous glass cases are counted
as research, not as ambiguous diagnostics. Every case id above comes from
`SUBJECTS` and the `composite` background arms in
`core/tests/common/selection_quality.rs`.

**4. Corpus scope is otherwise unchanged.** The 24 photographic crops and the 42
full images stay whole in the automatic report: all 24 and all 42 are pass-rule
cases.

**5. Opaque rule.** This replaces v1's `opaque_changed_pixels == 0`. At pixels
where the reference alpha is exactly 255, a pixel is *changed* if any channel of
`round(estimated * 255)` differs from the corresponding channel of
`<case>-source.png` — the definition already in
`core/tests/common/matting_quality.rs:212-218`. Gate v2 passes the opaque clause
iff all three hold:

- **(a)** the maximum per-channel `|delta|` over changed reference-opaque pixels
  is at most **2 code levels**;
- **(b)** `opaque_changed_pixels / opaque_pixels` is at most **0.01**;
- **(c)** every changed reference-opaque pixel lies in the existing Rust
  `boundary` set (`core/tests/common/matting_quality.rs:196-201`): some pixel
  within Chebyshev distance ≤ 2, clipped to the image, has `truth-alpha != 255`.

The owner's 1% stands; the 2% adjustment considered while drafting is withdrawn
(see "Honesty clause" below).

*Why (c) is the Rust boundary set and not "near a fractional pixel".* The
`hard_product` subject has an exactly 0/255 outline everywhere except the radius-5
ellipse hole: `core/tests/common/selection_quality.rs:58-66` uses integer bounds
for body, top and antenna, and the 4×4 area sampling at `+0.125…+0.875` therefore
yields exact 0 or 1 along those edges. A fractional-only neighbourhood would make
every change adjacent to that hard edge fail (c) even at a one-code delta, holding
a pass-rule subject to v1-exact by construction — the opposite of the rule's
intent. The `boundary` set ("soft, or the reference alpha varies within 2 px") is
the neighbourhood the rest of the gate already uses for its recomposition
boundary MAD, so (c) now uses it too. Measured effect: V1
`hard_product-light-binary` has 4 changed pixels at delta 1, all inside the
boundary set and none within 2 px of a fractional pixel — a v2 pass under the
frozen rule and a v2 failure under the fractional-only wording.

**6. Everything else is identical to v1.** `alpha_gate` (boundary MAD
non-regression within 1/255 and soft-detail recall ≥ 0.95) is unchanged. The
`color_gate` is unchanged except for the opaque clause: alpha-weighted foreground
MAD non-regression within 1/255, fractional foreground MAD ≤ 0.05, the 20%
reduction rule where the initial fractional error exceeds 0.02, and replacement
boundary MAD ≤ 0.02 on white, dark and checker. `has_fractional_reference` is
still required, and a refused alpha or foreground candidate still fails.

### Measurement source, in precedence order

1. `after.opaque_changed_max_code_delta` and `after.opaque_changed_outside_boundary`
   when present in the coupled record. New runs from the rebuilt evaluators always
   have them.
2. The decoded planes `<case>-{source,estimated-foreground,truth-alpha}.png`.
   Preserved records export these for 35 cases per candidate: all 24 analytic, the
   4 `photo-*-light-blurred` crops and the 7 `full-*-light-blurred` images.
3. `opaque_changed_pixels == 0` ⇒ the opaque rule passes. No decode is needed:
   with nothing changed, (a), (b) and (c) are all satisfied.
4. Otherwise the case verdict is **indeterminate** — never a v2 pass. It is listed
   per case with the reason "no exported planes and no v2 metric fields; only the
   changed fraction is known".

### Honesty clause: gate v2 does not admit the V5 hair case

The 1% → 2% relaxation considered while drafting was motivated by V5
`portrait_hair-light-binary`, whose alpha is nearly exact (boundary MAD 0.0003,
soft-detail recall 1.0). Measured by the re-scorer from that candidate's exported
planes:

| clause | value | verdict |
| --- | --- | --- |
| (a) max per-channel code delta | 2 | passes (≤ 2) |
| (b) changed fraction | 350 / 20,676 = 1.69% | **fails** (> 1%) |
| (c) changed pixels outside the boundary set | 249 of 350 | **fails** (> 0) |

The case fails (c) regardless of the changed-fraction bound: 249 of its 350
changed opaque pixels have no non-255 reference alpha within 2 px, which is
interior damage — exactly what the opaque rule exists to catch. Decision 2's
original rationale was therefore wrong for this case, the 2% bound would have
admitted nothing, and gate v2 does not admit V5 `portrait_hair-light-binary`.
The same holds for V5's other changed pass-rule cases: `portrait_hair-light-blurred`
(721 changed, 525 outside, max delta 4), `fur-dark-binary` (873 changed, 357
outside, max delta 2) and `hard_product-light-binary` (only 12 changed pixels at
delta 1 — clauses (a) and (b) both pass easily — but all 12 lie outside the
boundary set, so (c) fails). That last case shows clause (c) is not a formality:
it separates edge rounding from isolated interior damage at the same magnitude.

## Assisted completion subset — 36 cases, declared now

Completion of 9I means: **all 36 cases below pass gate v2 under the assisted
(brush-guided) protocol.** The automatic path is reported, never gating.

- All 24 `photo-*` crops: `photo-0051115Q_000001_{0003,0031,0060,0088}-{light,dark,checker}-{blurred,binary}`.
- For each of the six non-bottle full images — `fur-longhair`, `hard-arduboy`,
  `portrait-0051115Q_000001_{0003,0031,0060,0088}` — the two lexicographically
  first case ids of its six: `full-<image>-checker-binary` and
  `full-<image>-checker-blurred`. That is the deterministic rule, declared before
  any annotation, and it represents every subject category present in the full
  corpus except the bottle.
- `full-glass-bottle-*` stays excluded, as
  [assisted protocol v1](phase-9i-assisted-protocol-v1.md) already declared: the
  bottle silhouette is not transparent-interior truth.

The subset was verified against the 42 ids in a preserved `full/metrics.json` and
the 24 ids in a preserved `analytic-crops/metrics.json`; it is serialized in
`rescore.json` under `assisted_completion_subset`.

**Annotator clause.** The owner is the annotator. Assisted protocol v1's blinding
requirement applies to them for cases they have not inspected. Before the first
annotation the owner lists, in the run declaration, every case id they have already
inspected — source, reference, comparison panel or scores. Those cases are
annotated unblinded and flagged `annotator_exposed: true` in the per-case record.
All 36 stay in the denominator; exposure is disclosed, never an exclusion.

## Information-bound analysis

This is the durable form of the STOP analysis that closed the previous 9I run. It
explains why a rescope, rather than another candidate, is the honest next move.

### 1. The v1 gate was all 90 cases, combined

`scripts/phase9i_soft_prior_qualification.py` required
`cases == end_to_end_passes` over analytic 24, crops 24 and full 42;
`scripts/phase6_matting_qualification.py` required 24/24 analytic **and** 24/24
crops. End-to-end means the alpha gate and the coupled color gate on the same
case. Across V1–V5 the union of cases ever passed end-to-end is 8 distinct cases
of the 48 analytic + crop population. No ambiguous case, no glass case, no
checker-background case and no photographic binary start has ever passed.

### 2. The ambiguous analytic variant carries no recoverable color evidence

`core/tests/common/selection_quality.rs:88-100` authors the foreground as
`[0.24 + 0.03·sin(0.13x), 0.18, 0.12]` and the ambiguous background as
`[fg_r + 0.015·sin(0.12y), 0.18, 0.12]`. The difference is
`[−0.015·sin(0.12y), 0, 0]`: G and B are identical, and R differs by at most 3.82
code levels. On 11 of the 141 strand rows (52, 53, 78, 79, 104, 105, 130, 131,
156, 157, 158) `|ΔR|` is below half a code; on 23 rows it is below one code. After
u8 rounding the best attainable alpha resolution is about 0.26 per code, and on
those rows alpha is simply undefined by the pixels.

The consequence is visible in every preserved report. Each non-identity candidate
regresses boundary MAD on all six ambiguous cases (before → after): V2
hair-blurred 0.1417 → 0.1754, fur-blurred 0.1279 → 0.1874, glass-blurred 0.0340 →
0.0497, product-blurred 0.2453 → 0.2928, hair-binary 0.0939 → 0.1156, fur-binary
0.0712 → 0.1231, product-binary 0.0017 → 0.1996; V5 lands at 0.27–0.38. The
guided baseline in [phase-6 selection quality](phase-6-selection-quality.md)
shows the same shape: hair-ambiguous-blurred 0.1417 → 0.2427,
product-ambiguous-thresholded 0.0017 → 0.3871.

The identity candidate V4 passes alpha on the three blurred-ambiguous cases and
still fails color, because the blurred prior would have to be sharpened about 5×
to reach recomposition boundary MAD ≤ 0.02 (white-background boundary MAD 0.1160
hair, 0.1051 fur, 0.0386 glass). The only route to that sharpening is
deconvolving the fixture's own known radius-3 Gaussian, which is fixture-recipe
exploitation rather than a product algorithm and violates the "no truth-derived
input" rule in [phase-9i next steps](phase-9i-next-steps.md). The binary-ambiguous
cases additionally need ≥ 95% soft-detail recall from a thresholded prior whose
measured recall is 0.41 (hair), 0.48 (fur) and 0.40 (product), with no color
signal to recover it from.

No algorithm and no human can matte an edge that is not in the pixels. These six
cases measure the fixture, not the product, so they leave the pass rule and stay
as reported diagnostics.

### 3. Binary glass has no foreground evidence on hand

`core/tests/common/selection_quality.rs:47-57` gives the glass interior an alpha
between 0.16 and 0.51 with a 0.7 rim three pixels wide. The binary prior retains
only those rims and a few stripes: soft-detail recall 0.0916 against a 0.95 gate.
The best result ever measured is V2 at 0.143 / 0.146 / 0.236; V5, the closed-form
candidate and the sampled candidate all refuse the six glass cases outright.
[The cutout status](phase-9i-cutout-status.md) ("Remaining quality failures") and
[phase-9i next steps](phase-9i-next-steps.md) ("Separate glass and transmission
research") already assign this to a separate track that needs licensed
photographic transmission references. None were obtained, and no contact or
purchase is authorized. Glass therefore moves to that track rather than blocking
a product gate.

### 4. Exact-opaque-RGB and decontamination are in direct tension

V5 `portrait_hair-light-binary` has boundary MAD 0.0003 and recall 1.0 — alpha
PASS — and fails color solely on 350 of 20,676 reference-opaque pixels moving by
at most 2 code levels (minimum alpha at those pixels 253).
`hard_product-light-binary` fails on 12 changed opaque pixels. V3's source
protection cut opaque damage to 3/24, 0/24 and 3/42 but raised
replacement-boundary failures to 22/24, 21/24 and 33/42
([cutout status](phase-9i-cutout-status.md)). `core/tests/foreground_ambiguity.rs`
proves the underlying obstruction: `identical_composites_and_valid_constraints_admit_conflicting_foreground_truths`,
`exact_reconstruction_and_gamut_feasibility_do_not_identify_foreground_color` and
`foreground_color_stage_with_estimated_alpha_does_not_certify_opaque_preservation`
each show the opaque-versus-translucent decision is underdetermined from RGB,
prior and labels. A bit-exact opaque criterion therefore measures alpha rounding,
not visible damage — hence the bounded rule, with clause (c) keeping genuine
interior damage a failure.

### 5. The project had already recorded a stop decision

[The feasibility decision](phase-9i-feasibility-status.md) says: "Do not download
weights, run an undeclared model, tune V5 again, enable controls or mark the
original quality gate complete", and "This milestone does not add a model to
Rasterize or begin a sequence of further custom matting variants."
[Phase-9i next steps](phase-9i-next-steps.md)'s decision table says: "Same
interior damage, preservation/decontamination tradeoff or resource refusal → Stop
that algorithm line." Its next automatic comparison was gated on RVM checkpoint
terms, and the assisted protocol on genuinely blinded human annotation. Both
conditions are addressed by this document: RVM is dropped (decision 5) and the
annotator clause above defines what blinding means for this owner.

### 6. The corpus was purged and has been re-acquired

macOS purges `/private/tmp` entries untouched for three days, which emptied the
Phase 6 corpus on 2026-09-14/15 and made the runners' verify stages fail before
any candidate could be scored. The corpus now lives under
`~/devel/rasterize-fixtures/phase6/` and is complete; see "Persistent storage".

## The six owner decisions

**Decision 1 — analytic case scope.** Ambiguous cases leave the pass rule and stay
as reported diagnostics; glass cases leave the pass rule and join the
transmission-research track. *Reason:* those fixtures carry no recoverable
evidence (sections 2 and 3), so they measure the fixture rather than the product.
The exact case ids are listed above and are derived from the fixture source.

**Decision 2 — bounded opaque rule.** "Preserve opaque source RGB exactly" becomes
clauses (a), (b) and (c). *Reason:* under v1 a two-code alpha rounding error at
one opaque pixel in twenty thousand fails a case whose alpha and boundary both
pass. *Recorded correction:* the motivating case does not survive the new rule
either (see the honesty clause), the bound stays at the owner's 1%, and clause
(c) uses the Rust boundary set for the `hard_product` reason given above.

**Decision 3 — every other threshold is unchanged.** Boundary MAD ≤ 0.02 on all
three backgrounds, fractional foreground MAD ≤ 0.05 with the 20% reduction rule,
alpha-weighted MAD non-regression, soft-detail recall ≥ 0.95, false-positive
alpha. Refusals still count as failures.

**Decision 4 — completion criterion.** 9I completes when the assisted protocol
passes gate v2 on the frozen 36-case subset declared above; the automatic path is
reported, not gating. The owner annotates, under the annotator clause. This record
satisfies the documents' existing "changed completion scope must be explicit"
clause.

**Decision 5 — RVM is dropped.** The repository's weight-license issue is
still open and unanswered as of 2026-09-15. The code is GPL-3.0 and Rasterize is
unlicensed (all rights reserved), so Rasterize could not link it even with an
evaluation grant for the weights. RVM is also human-video trained and trimap-free,
so it cannot consume brush guidance — the one input the assisted protocol adds.
No neural matting model is added. Apple Vision stays the automatic prior. This is
the closing decision of work package 1 and supersedes the conditional selection in
[the feasibility decision](phase-9i-feasibility-status.md) and
[the established pipeline decision](phase-9i-established-pipeline-decision.md);
the three-candidate survey, its hashes and the FBA/BGMv2 findings stand as
history.

**Decision 6 — persistent storage.** Every canonical corpus and report path moves
from `/private/tmp` to `~/devel/rasterize-fixtures/`. See below.

## Persistent storage

**Preserved candidate reports** — `~/devel/rasterize-fixtures/phase9i-reports/`:

| directory | contents |
| --- | --- |
| `rz-phase9i-soft-prior-v1` | declaration, four stage logs, all 90 records, exported planes, failure diagnostics |
| `rz-phase9i-soft-prior-v2` | the same, plus `extent-audit.json` |
| `rz-phase9i-soft-prior-v3` | source-only foreground protection, all 90 records |
| `rz-phase9i-soft-prior-v4` | stable-prior comparison, all 90 records |
| `rz-phase9i-joint-bayesian-v5` | joint Bayesian candidate, all 90 records |
| `rz-phase9i-v5-diagnostics` | V5 artifact/metric audits and their scripts |
| `rz-phase9i-feasibility-root-review` | `inventory.json`, `original90-manifest.json` |
| `rz-phase9i-feasibility-evidence-audit-final` | `inventory.json`, `original90-manifest.json` |
| `gate-v2` | this milestone's `rescore.json` and `rescore.md` |

**Corpus** — `~/devel/rasterize-fixtures/phase6/`, all three directories complete
and verified on 2026-09-15 with the repository's own fixture scripts:

| directory | acquisition log | `--verify-only` result |
| --- | --- | --- |
| `references` | `references.log`: "Verified 4 photographic foreground/alpha references", `references exit 0` | "Verified 4 photographic foreground/alpha references", exit 0 |
| `photos` | `photos.log`: four pinned hashes, `photos exit 0` | not applicable (no verifier) |
| `full-references` | `full-references.log`: "Verified 7 full-image reference photographs and their permission evidence", `full-references exit 0 (after 90s wait)` | "Verified 7 full-image reference photographs and their permission evidence", exit 0 |

Nothing was downloaded in this milestone; only the two `--verify-only` commands
were run.

**Purge note.** Every `/private/tmp/rz-*` receipt that is not in the preserved
list above was purged by macOS and is gone. That includes
`rz-phase9i-parity-final`, `rz-phase9i-stability-audit`,
`rz-phase9i-continue-parity`, `rz-phase9i-continue-quality-audit.json`,
`rz-phase9i-rvm-source-audit-*`, `rz-phase6c-qualified`,
`rz-phase6c-guided-baseline`, `rz-phase6-full-quality`, `rz-phase9i-target` and
the various `rz-phase6-*.log` build/test logs. Older documents still cite those
paths as historical receipts; they are records of what was measured, not
retrievable files. Re-running a purged check needs a new output directory under
`~/devel/rasterize-fixtures/` and a new receipt.

## The test-support metric

`color_metrics` in `core/tests/common/matting_quality.rs` now emits two additional
fields in the same per-pixel loop that already computes `opaque_changed_pixels`:

- `"opaque_changed_max_code_delta"` (u8) — the maximum over changed
  reference-opaque pixels and channels of
  `|round(estimated * 255) as u8 − source|`; 0 when nothing changed.
- `"opaque_changed_outside_boundary"` (usize) — the number of changed
  reference-opaque pixels whose existing `varying` flag is false, i.e. the
  clause-(c) count.

`color_gate`, `color_gate_contract()` and every existing field are untouched, and
the report `schema_version` stays 1: this is additive test-support data, not a new
gate in Rust. The module reaches every evaluator through `#[path]`
(`core/examples/phase6_matting_quality.rs:20`,
`core/examples/phase6_selection_quality.rs:15`, and through
`core/examples/phase6_full_image_quality.rs:9` into the soft-prior, Bayesian,
closed-form and external-matte evaluators), so any future run is fully
v2-decidable without decoding a single PNG. Two hand-computed assertions in
`core/tests/matting_qualification.rs` pin the values (26 and 0 on the changed
fixture, 0 and 0 on the exact one).

`predeclared.json` in every preserved run hashes this file. The preserved hash is
`1f047ac849d0ea3562ed4a6c29e6a05b67d2f767e0e7e4b2a8e42521598686e6`; the current
file hashes to
`0c5963ff8ffd670b9ec681d600de7889289b9975f93d6025433dc69e8d99b680`. That change
is expected and is why the preserved records do not carry the two new fields: they
predate them. A future run's `predeclared.json` will pin the new hash.

## Re-scoring the five preserved candidates

`scripts/phase9i_gate_v2_rescore.py` (stdlib only, pure-Python PNG decode through
`scripts/verify_phase2.py`'s `read_png`, no pixel cap and no rescaling) scored all
five preserved candidates under both gates at repository head
`b2e0c8080c7e094515025de13d4c4d2130674d5a`. Each candidate contributed exactly 90
cases (24 analytic / 24 crops / 42 full). For every one of the 450 case records the
re-scorer's recomputed v1 verdict equals the Rust-recorded `passes_end_to_end_gate`,
and every plane-derived changed count equals the recorded `opaque_changed_pixels`;
both are assertions in the script, so the run would have failed otherwise.

| candidate | analytic | crops | full |
| --- | --- | --- | --- |
| v1 | 1 / 3 | 0 / 0 | 0 / 0 |
| v2 | 1 / 3 | 0 / 0 (+6 indeterminate) | 0 / 0 (+3 indeterminate) |
| v3 | 1 / 1 | 3 / 3 | 4 / 4 |
| v4 | 0 / 0 | 3 / 3 | 4 / 4 |
| v5 | 2 / 2 | 0 / 0 | 0 / 0 |

Cells are v1 passes / v2 passes. v1 counts every case in the corpus; v2 counts only
gate v2 pass-rule cases (analytic drops 6 ambiguous diagnostics and 6 glass
transmission-research cases; crops 24 and full 42 stay whole).

Receipts: `~/devel/rasterize-fixtures/phase9i-reports/gate-v2/rescore.json`,
SHA-256 `a8b788b862ea8922cb815ab7e95bb308853db1e16be336e710a19caf8e961ac7`, and
`rescore.md`, SHA-256
`c7f28a99b213d9c540c4bad603ee1d43404666c9f8c7d9433189bf08c1da5c35`. The table
above is `rescore.md` verbatim. The JSON pins the SHA-256 of each scored
`metrics.json`, the frozen gate v2 constants, the 36-case subset and every per-case
verdict with its opaque measurement and its measurement source. The two
`rz-phase9i-feasibility-*` inventory directories were skipped: they hold inventory
manifests, not candidate metrics. `rz-phase9i-v5-diagnostics/artifacts-final.json`
may be cited as a cross-check but was not used as an input.

What the numbers mean:

- Gate v2 admits **four** analytic pass-rule cases that gate v1 rejected, all of
  them on the bounded opaque rule alone: V1 `fur-light-binary` (9 changed pixels,
  max delta 1, 0.048%, 0 outside the boundary set) and V1
  `hard_product-light-binary` (4, delta 1, 0.013%, 0 outside); V2 `fur-dark-binary`
  (22, delta 1, 0.118%, 0 outside) and V2 `hard_product-dark-binary` (2, delta 1,
  0.007%, 0 outside). These are the alpha-rounding failures decision 2 was aimed at.
- Gate v2 admits nothing on the photographic corpora. V3 and V4 keep their three
  crops and four full images; V1, V2 and V5 keep none.
- The union over all five candidates of analytic pass-rule cases that pass gate v2
  is six of twelve: `portrait_hair-light-binary`, `fur-light-blurred`,
  `fur-light-binary`, `fur-dark-binary`, `hard_product-light-binary`,
  `hard_product-dark-binary`. No single candidate passes more than three.
- Nine cases are **indeterminate**, all in V2: six crops
  (`photo-*-{dark,checker}-blurred`, changed fractions 0.50%–0.81%) and three full
  images (`full-fur-longhair-{dark,checker}-blurred` at 2.19% and 1.19%,
  `full-portrait-0051115Q_000001_0031-dark-blurred` at 0.27%). Those records changed
  some opaque pixels, export no planes and predate the new metric fields, so
  clauses (a) and (c) cannot be evaluated. They are reported as indeterminate and
  counted as non-passes; two of the three full images would fail clause (b) on the
  changed fraction alone, but that is not enough to call the rest a pass.
- Measurement sources across the 450 verdicts: 76 from decoded planes, 184 from
  `opaque_changed_pixels == 0`, 96 refused before any color measurement, and 94
  with no opaque measurement available. Only 9 of those 94 are decision-relevant
  (the indeterminate list above); the other 85 already fail another gate v2 clause.

## What this milestone does not do

- It **does not qualify any candidate.** No candidate passes gate v2 on any corpus.
- It **does not enable any experimental control**, change `core/src`, Swift, the
  FFI header or `RasterCore.swift`, or alter `Rasterize.xcodeproj`.
- It **does not make the preserved records fully v2-decidable.** A preserved record
  is decidable only where planes were exported (35 cases per candidate) or where
  nothing changed. Nine cases remain indeterminate and can never be resolved
  without re-running the candidate.
- Runs from the rebuilt evaluators **are** fully decidable: `color_metrics` emits
  the clause-(a) and clause-(c) numbers for every case, so `v2_indeterminate` is 0
  for a new run. The runners print it beside `numerical_gate_passed` so a reader
  never has to assume.
- It **does not change what Rust records.** The evaluators still write the v1
  `passes_end_to_end_gate`, and `color_gate` / `color_gate_contract()` /
  `summary_for` are untouched. Only the Python runners' pass decision moved, and
  `--gate v1` reproduces the old decision exactly.
- It **does not collect or score any assisted annotation.** The workspace, the
  replay tool and the assisted run are later work.

## Statements superseded by this document

- [phase-6-matting-reference-protocol.md](phase-6-matting-reference-protocol.md),
  "Independent color measurements": "preserve opaque source RGB exactly" is gate
  v1. Gate v2 supersedes it for 9I completion with clauses (a)/(b)/(c).
- [phase-6-selection-mask-status.md](phase-6-selection-mask-status.md),
  [phase-7-carryovers.md](phase-7-carryovers.md) (P7-12) and
  [professional-photo-editor-implementation-plan.md](professional-photo-editor-implementation-plan.md)
  (9I): "the original quality criteria" now means gate v2 with the assisted
  completion criterion.
- [phase-9i-feasibility-status.md](phase-9i-feasibility-status.md) and
  [phase-9i-established-pipeline-decision.md](phase-9i-established-pipeline-decision.md):
  RVM's conditional selection is closed as "dropped". Work package 1 is closed with
  no pipeline selected.
- [phase-9i-assisted-protocol-v1.md](phase-9i-assisted-protocol-v1.md): the
  protocol keeps its 78-case ordinary-edge population and its 78/78 whole-run
  report. The completion decision for 9I is the 36-case subset above, the "exactly
  equal" opaque bullet is replaced by clauses (a)/(b)/(c), and the annotator clause
  above defines blinding for this owner.
- Every document citing a `/private/tmp/rz-phase6-*` or `/private/tmp/rz-phase9i-*`
  path as the canonical corpus or report location: the canonical locations are
  under `~/devel/rasterize-fixtures/`.

## Reproduction

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/phase6_reference_fixtures.py \
  ~/devel/rasterize-fixtures/phase6/references --verify-only
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/phase6_full_reference_fixtures.py \
  ~/devel/rasterize-fixtures/phase6/full-references --verify-only
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/phase9i_gate_v2_rescore.py
cd core && cargo test --release --test matting_qualification && cd ..
cd core && cargo build --release --example phase9i_soft_prior_quality && cd ..
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/phase9i_soft_prior_qualification.py \
  ~/devel/rasterize-fixtures/phase9i-reports/<new-run> --gate v2 \
  --executable core/target/release/examples/phase9i_soft_prior_quality \
  --crops ~/devel/rasterize-fixtures/phase6/references \
  --full ~/devel/rasterize-fixtures/phase6/full-references
```

The re-scorer refuses an output path inside the repository. No photograph, PNG or
JSON report from any of these runs enters the repository.
