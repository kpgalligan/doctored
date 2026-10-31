# Source-constrained content-aware fill (ADR)

- **Status:** Accepted — closed (verifies P7-18)
- **Phase / dates:** 9E, September 2026
- **Original:** [archive/phase-9e-content-aware-status.md](../archive/phase-9e-content-aware-status.md) — full history, measurements, receipts
- **Key commits:** `694c39f` (Complete content-aware workflow, phase 9E)

## Context
Content-Aware Fill had to work at both document depths, let the user constrain where source texture may
come from, choose where output goes, and inspect the actual result before committing — all without a
second synthesis implementation and without changing legacy byte output. It builds on the typed
PatchMatch/membrane path qualified in [phase-9d-synthesis-qualification.md](phase-9d-synthesis-qualification.md).

## Decisions
- **Precision.** The typed specialization carries source, pyramid, votes and solve RGB in native-code f32
  and quantizes once at writeback. Source plates come from captured document pixels under the
  Current / Current and Below / All Layers / ignore-adjustment semantics; a displayed image never supplies
  source colour.
- **Selection coverage.** The u8 selection interface keeps its meaning (promotion is ×257, no new precision
  claimed); typed core callers keep genuine u16 coverage including 1. Coverage 0 and destination alpha are
  exact.
- **`source_mask` recipe, version 1:** `initial: "all" | "none"` plus ordered `include_rect`, `exclude_rect`,
  `include_brush`, `exclude_brush` (rects x/y/w/h; brushes radius + `[[x,y],…]` joined by round capsules via
  the shared capsule geometry, rasterized in Rust — `core/src/inpaint_source_mask.rs`). It changes
  **eligibility only**, never source RGB or the fill selection. Omitted recipe ⇒ legacy byte output;
  explicit empty all-source recipe ⇒ identical result.
- **Eligibility is ANDed at every pyramid level**, separate from boundary/context data, so an excluded pixel
  cannot vote through a coarse patch; real boundary pixels still guide matching and illumination.
- **Recipe limits:** ≤512 operations, ≤4096 brush points, ≤256 M geometry visits, request ≤64 KiB, radius
  0.5…4096, finite coordinates within ±1,000,000. The 9D planner caps (1 M hole, 4 M dilated, 80 M window
  pixels, single-window solve) remain; memory admission may refuse earlier. (The 1 M hole cap has since
  ended; see "Later changes".)
- **Output:** replace in place, or a new Content-Aware Fill layer directly above the target (solved against
  the current-and-below destination plate while reading the requested sampling plate). Soft coverage is
  applied once. Gated enclosing groups refuse new-layer output. Masks, scalar planes, saved channels and
  Quick Mask refuse this RGB operation.
- **Exact final comparison.** Prepare yields one admitted immutable candidate; Apply publishes *that*
  candidate through central history without re-running synthesis. Any parameter/geometry change
  invalidates it. Validation covers document identity/revision, selection, active/selected layers,
  target, Quick Mask and owner lifetime.
- **MCP:** `preview_content_aware_fill` prepares a single-use ticket (5-minute expiry, refuses conflicting
  parameters); passing `preview_id` to `content_aware_fill` accepts it;
  `discard_content_aware_fill_preview` cancels. Legacy ring/seed/sampling aliases and
  `background`/`job_status`/`cancel_job` behaviour stay. Actions record one resolved `content_aware_fill`
  step with source geometry and output choice — never a ticket; nested geometry counts against the
  Actions point budget.
- **Patch tool / `patch_region` at 16 bits** use D's captured retouch-heal path: Source/Destination,
  rounded integer offsets and in-place-only output keep their meaning; the gesture latches
  source/mode/direction at placement and refuses stale publication. Byte Patch keeps its foreground solver.
- **Lifecycle:** stale acceptance releases the candidate; the busy guard runs before replacing a prior
  ticket; cancel/replace/expire/close clear candidate references.

## Rejected alternatives
- Re-running synthesis on Apply — the committed result could differ from what the user inspected.
- Treating an exclusion as "remove this from the photo" — exclusion affects sampling only.

## Consequences
- Qualified for small holes: two pinned photographs at both depths (mean error below 15 byte-code
  equivalents, <25% corruption error remaining) with different excluded contaminants yielding identical
  repairs. The 24×24 periodic-hole phase ambiguity from 9D remains; comparison lets the user inspect.
- UI qualification was at backing scale 1 only; no 2× or broad monitor claim.
- New synthesis-adjacent features must keep eligibility separate from context and publish only the
  inspected candidate.

## Later changes
- **Holes over 1 MP (October 2026).** A call covering more than 1 MP is no longer refused. Its search runs at 1/2^L
  scale (L = 1 to 4 MP, 2 to 16 MP, 3 to 64 MP; `doc_inpaint::inpaint_lift`), the finer levels copy source pixels
  through the doubled field without searching, and the 4 M dilated cap is multiplied by 4^L. Still refused: one
  part's window over 32 MP, windows over 80 MP, the scaled dilated cap, more than 1,000,000 separate parts, a part
  too narrow to reduce that holds over 1 MP to search, and memory admission (formulas unchanged). Structure larger
  than about 7·2^L px is matched; finer texture is real source texture but is not matched in phase to the hole's
  edge. Above 1 MP only synthetic pictures were checked (flat, ramp, random cells, a corner-to-corner band); no
  photograph was qualified, and the periodic-hole limit below stands.
- **Depth rule.** The pyramid now stops per axis while a narrow pair of margins still holds a
  source patch, and the coarsest level's iteration count follows its hole's size
  (`patchmatch::should_coarsen`, `patchmatch::coarsest_iterations`). Byte and typed output changed at
  every hole size; "without changing legacy byte output" no longer binds this pipeline.
- **Remove and Content-Aware Move**, at 8 and 16 bits. Remove (`remove_stroke`) makes the painted
  stroke the hole of one Content-Aware Fill at the automatic ring. Content-Aware Move
  (`content_aware_move`) fills the whole outlined region, then heals its original pixels in at the offset
  with Patch's Destination heal, as one job and one undo step; at 16 bits that heal reads the pre-fill
  picture through `dr_precision_doc_retouch_from`. Extend is the heal alone. Offsets stay whole pixels.

## Open items (verified 2026-09-26)
- `DROPPED` (2026-10-30) — large periodic-hole phase accuracy and large-removal photographic qualification: a
  declared PatchMatch limit with no planned approach; the comparison UI lets the user pick candidates.
- `DONE` — P7-12's other half: edge brushing and colour decontamination shipped 2026-10-11 in Select and Mask; the
  9I cutout gate is no longer a ship condition and its assisted session is dropped.
- `DONE` — Scalar targets and native selection creation (9G) — `f457cdb`.
