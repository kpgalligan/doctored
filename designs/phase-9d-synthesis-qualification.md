# Native spot-healing synthesis qualification (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 9D (spot-heal synthesis receipt), September 2026
- **Original:** [archive/phase-9d-synthesis-qualification.md](../archive/phase-9d-synthesis-qualification.md) — full history, measurements, receipts
- **Key commits:** `bf706bb` (Complete native retouch workflow, phase 9D)

## Context
The native (16-bit) Spot Healing Brush had to reuse the existing byte-depth synthesis stack — component
planner, PatchMatch pyramid/search/vote and membrane solver — without routing search, source colour or
solved RGB through a byte image, and without changing byte-depth output. This record qualifies that
typed specialization; the broader 9D retouch workflow is recorded in `phase-9-live-filters-status.md`.

## Decisions
- **One algorithm, typed carrier.** RGB stays in native-code f32 through pyramid, synthesis and solve; only
  final destination publication quantizes. *Why:* sub-byte texture must survive; no second synthesis
  implementation.
- **Byte path unchanged, bit for bit.** The byte SSD keeps its integer row kernel (a generic f64 loop cost
  46.5%); the typed row kernel uses f64. The shared nearest-neighbour distance array is now f64, so the
  byte window scratch allowance rose from 128 to 136 bytes/pixel.
- **Alpha rules.** Source sampling requires alpha ≥ 32768 (the byte half-alpha rule). Destination alpha is
  exact, including alpha 1; fully transparent destination storage and pixels outside the stroke's
  half-coverage contour stay unchanged. Native coverage 32767 is outside, 32768 inside the spot contour.
  Binary topology still uses the established contour code, distinct from RGB and from u16 write coverage.
- **Profile work happens upstream.** Source-profile conversion/composition is done by the retouch
  dispatcher before this pipeline receives its captured canvas plate.
- **Variance floor is one squared native code**, so sub-byte texture participates in matching (it does not
  inherit a 257-code noise floor).
- **Ceilings are computational, not quality claims:** one million hole pixels, four million dilated pixels,
  80 million cumulative window pixels, the single-window solve cap; native memory admission (≈320 bytes
  per largest-window pixel plus component storage and captured plates/output) may refuse earlier.
- **Refusals are atomic.** Mask/bounds mismatch, insufficient memory, precancellation and empty coverage
  never change the document; a source with alpha < 32768 and no complete patch refuses.
- **Public entry point:** `dr_precision_doc_edit` (exercised by
  [`core/tests/precision_inpaint.rs`](../core/tests/precision_inpaint.rs)).

## Rejected alternatives
- Exact periodic reconstruction as a gate for large holes — an initial 24×24-hole/128×128 period-four test
  failed (coarse pyramid levels lose texture phase); the gate was replaced by analytic-range, exact-outside
  and adjacent-code checks rather than reported as passed.
- A generic (non-integer) byte SSD loop — measured 46.5% slower.

## Consequences
- Small photographic spots are qualified (two pinned photographs, mean error below 15 byte-code
  equivalents and <25% of corruption error remaining, outside coverage exact); large removals and
  large periodic fills are **not** claimed correct in phase.
- Any change to the synthesis stack must keep the byte path byte-identical and the typed path
  quantize-once.

## Later changes
- **October 2026.** The one-million hole ceiling is gone. A call covering more is searched at 1/2^L scale (L = 1 to
  4 MP, 2 to 16 MP, 3 to 64 MP; `doc_inpaint::inpaint_lift`) and its four-million dilated ceiling is multiplied by
  4^L; the finer levels copy source pixels through the doubled field without searching. The other ceilings stand,
  with two new ones: 1,000,000 separate parts, and a part too narrow to reduce that holds over 1 MP to search.
  Large removals are still not qualified on photographs (synthetic checks only) and large periodic fills are still
  not claimed correct in phase.
- **The byte path is no longer bit-identical to 9D's.** `patchmatch::should_coarsen` stops the
  pyramid per axis while a narrow pair of margins still holds a source patch, and the coarsest level's
  iteration count follows its hole's size (`patchmatch::coarsest_iterations`). Both depths changed
  together, because a hole with one narrow pair of margins was filled from its wide sides only (column
  ramp, 1 MP hole, 300 and 150 px margins: 22.4 codes mean error before, 0.0 after at 8 bits and 0.003
  at 16). The typed path still quantizes once.

## Open items (verified 2026-09-26)
- `DONE` — Source exclusion editing, final candidate comparison and Content-Aware Fill output choices
  (handed to 9E) — see [phase-9e-content-aware-status.md](phase-9e-content-aware-status.md); commit `694c39f`.
- `DONE` — General scalar targets and authoritative native selection creation (handed to 9G) — see
  [phase-9g-selections-masks-status.md](phase-9g-selections-masks-status.md); commit `f457cdb`.
- `TRACKED` — Phase ambiguity of large periodic fills remains a declared limit → carryover P7-18 row
  ("large periodic-hole phase ambiguity … remain explicit 13A/13D limits").
