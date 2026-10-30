# Phase 9E — source-constrained content-aware fill

Phase 9E is complete within the scope below and verifies P7-18. Subsequent
[9F](phase-9f-selected-tone-status.md), [9G](phase-9g-selections-masks-status.md)
and [9H](phase-9h-paint-fill-status.md) qualification brings the current Phase 7
ledger to 18 verified assignments and 18 open. P7-12/9I and Phase 9 remain open.

## Source, precision and output contract

Content-Aware Fill now supports both document depths through the existing
PatchMatch and membrane algorithms. The typed specialization carries source,
pyramid, votes and solve RGB in native-code f32 and quantizes once at destination
writeback. Source plates come from captured document pixels and respect Current,
Current and Below, All Layers and ignore-adjustment semantics. A displayed image
never supplies committed source colour. The original u8 selection interface keeps
its existing meaning: promotion to u16 multiplies coverage by 257 and does not
claim additional selection precision. Core typed callers also retain genuine
u16 coverage, including coverage 1. Coverage zero and destination alpha are exact.

The optional `source_mask` is a version-1 recipe with `initial: "all"` or
`"none"` and ordered `include_rect`, `exclude_rect`, `include_brush` and
`exclude_brush` operations. Rectangles use x/y/width/height; brush strokes use a
radius and `[[x,y],…]` points connected by round capsules. Rasterization is in
Rust using the shared capsule geometry. Every operation changes eligibility,
not source RGB or the fill selection. The overlay shows allowed source areas;
actual sampling is also restricted by the ring, transparency and complete-patch
requirements.

Source eligibility remains separate from boundary/context data and reduces by
AND at every pyramid level. Thus an excluded pixel cannot enter a source vote
through a coarse source patch. Real boundary pixels still guide matching and
illumination; excluding an area is not an instruction to remove it from the
photograph. Omitting the recipe preserves legacy byte output; an explicit empty
all-source recipe produces the same result.

Recipes admit at most 512 operations, 4096 total brush points and 256 million
geometry pixel visits. The complete request is limited to 64 KiB. Brush radii
are 0.5…4096 pixels and coordinates are finite within ±1,000,000. The existing
one-million-pixel hole, four-million-pixel dilated area, 80-million-pixel summed
window and single-window solve caps remain. Memory admission may refuse earlier.
These are computation ceilings, not photographic quality guarantees.

Output can replace the target pixels or create a new Content-Aware Fill layer
directly above it. New output solves against the current-and-below destination
plate while reading the requested sampling plate. Soft selection coverage is
applied once, before depositing the changed result wherever coverage is positive.
The original layer and its editable description remain exact. Opaque backdrops
match in-place output; partial-alpha output keeps the established retouch
compositing behavior and can make a translucent backdrop more opaque. Gated
enclosing groups refuse new-layer output. Masks, scalar planes, saved channels
and Quick Mask explicitly refuse this RGB operation; their native editing paths
are qualified separately in [9G](phase-9g-selections-masks-status.md).

## Exact final comparison and automation

The sheet supports source inclusion/exclusion brushes and rectangles, selection
outline, Before/After, Fit/100% and pan. Preparing produces one admitted immutable
candidate and its native/display projection. Apply publishes that exact candidate
through central history, without running synthesis again. Changing parameters or
source geometry invalidates it. Failures are visible; a failed preview cannot be
accepted as an unchanged picture.

Candidate validation includes document identity/revision, selection, active and
selected layers, target, Quick Mask and owner lifetime. Cancel, replacement,
expiration and document close clear candidate references; a cancelled worker
retains only its required inputs until its mailbox drains and releases the shared
worker slot. Native progress/cancellation and memory reservations remain visible.

Ordinary `content_aware_fill` calls preserve legacy ring/seed/sampling aliases and
the existing `background`, `job_status` and `cancel_job` behavior. New
`preview_content_aware_fill` prepares a candidate; `preview_id` polls its state
and returns Before/After display PNGs once ready. Passing that ID to
`content_aware_fill` accepts the exact result. The ticket is single-use, expires
after five minutes and refuses conflicting parameters. An explicit
`discard_content_aware_fill_preview` cancels it. Actions record one resolved
`content_aware_fill` step with source geometry and output choice, never a ticket.
Nested source geometry counts against the existing Actions point budget.

The Patch tool and `patch_region` now also support native documents through D's
captured retouch-heal path. Source/Destination directions, established rounded
integer offsets and in-place-only output retain their meaning. The actual gesture
latches source/mode/direction at placement, renders from that capture and refuses
stale publication. Polygon and adopted-selection Actions replay through the same
path. Native Patch is admitted and cancellable; byte Patch retains its existing
foreground solver and byte overlay policy. This does not introduce a second
synthesis implementation.

## Qualification

Independent core checks:

- Eight workflow FFI groups cover legacy byte equivalence, source restrictions,
  brush exclusion/re-inclusion, request limits, native soft coverage against a
  separate f64 reference, tiny alpha, output equivalence, source immutability and
  cancellation after native stage 3 with scratch released.
- Sixty-one targeted regression tests pass, including the earlier byte/typed
  synthesis, retouch and shared capsule workflows. All-target Clippy passes.
- The photographic test in `core/tests/content_aware_photographs.rs` passes four
  cases: two real photographs in both depths, with different unwanted source
  textures under the same exclusion producing identical repaired samples.

The photographic sources are the same pinned images recorded in the
[9D synthesis receipt](phase-9d-synthesis-qualification.md):

| Source | Dimensions | SHA-256 |
| --- | --- | --- |
| Astronaut | 512×512 | `88431cd9653ccd539741b555fb0a46b61558b301d4110412b5bc28b5e3ea6cb5` |
| Fur | 451×300 | `596aa1e7cb875eb79f437e310381d26b338a81c2da23439704a73c4651e8c4bb` |

Their native variants use that receipt's explicitly generated sub-byte carrier;
the photographs themselves are not relabelled native camera captures. Each fill
replaces a radius-four disc (49 pixels), at (197,122) for astronaut and (220,68)
for fur. The excluded contaminant is an 8×16 rectangle at (centre x+12, centre y−8):
inside the sampling ring but outside the target-patch and membrane boundary
context. Its two RGB variants are (60001,3001,50003) and (3001,55003,9001), scaled
to byte codes for the byte case. Every outside pixel and alpha sample stays exact.

The fixed gates were mean absolute RGB error below 15 byte-code equivalents and
remaining error below 25% of the original corruption. Both were chosen before
observing these results; comparison uses the uncorrupted photograph, not another
fill.

| Photograph / depth | Mean absolute error | Corruption error remaining | Excluded variants |
| --- | --- | --- | --- |
| Astronaut / 8 | 8.7551 /255 | 10.943% | Exact |
| Astronaut / 16 | 9.6441 /255 | 12.041% | Exact |
| Fur / 8 | 9.0000 /255 | 8.247% | Exact |
| Fur / 16 | 8.9938 /255 | 8.234% | Exact |

The previously recorded 24×24 periodic-hole phase ambiguity remains a limitation:
native precision and source eligibility do not guarantee exact reconstruction of
ambiguous repetitive texture. Final-result comparison lets the user inspect the
actual answer before committing. Broader large-image photographic quality remains
13A/13D; native selection creation and scalar editing are qualified separately
in [9G](phase-9g-selections-masks-status.md).

## Final release and lifecycle receipts

All final checks pass against the frozen product sources:

| Check | Result |
| --- | --- |
| Full Rust regression | 1,299 passed, zero failed, two existing ignored, 131 result groups; photographic environment enabled |
| Rust formatting and all-target Clippy | Pass, including `-D warnings` |
| Authoritative Swift typecheck | Pass, zero errors |
| Production `make app` | `BUILD SUCCEEDED`, normal Release LTO |
| Owned Release MCP workflow | 16 groups across both depths; 763 source inputs unchanged with exact file set |
| Owned Release lifecycle | Eight groups across both depths; no source changes |
| Actual-source AppKit UI | 12 groups: four CAF depth/output combinations plus eight Patch depth/direction/region combinations; 379 inputs unchanged, 376 compiled |
| Public MCP reference | Regenerated from the owned final Release app; exact second live comparison, 180 tools |
| Independent integration review | No additional confirmed findings after the lifecycle corrections below |

CAF UI qualification sends real source rectangle and comparison control events,
checks exact before/after projections, changes the seed and prepares again, and
proves Fit/100%/pan do not recompute the candidate. Apply publishes the very same
candidate owner and produces one Action and one Undo transition, with exact
Undo/Redo and MCP Action replay. Cancel and actual document close cancel the
review, clear its displayed images and drain the worker. Scalar-target validation
uses the editor's real target setter. Screenshots were inspected with all source
and comparison controls visible. UI display qualification is at backing scale 1;
this is not a new 2× or broad monitor claim.

Patch UI tests use an independently defined affine background and a damaged
interior with a clean membrane boundary. Centre reconstruction stays within four
u16 codes or two byte codes; all outside samples and alpha remain exact. Source
and Destination, polygon and adopted elliptical selection, changed preferences
mid-drag, stale capture, cancel, one Action, Undo/Redo and exact MCP replay all
pass. The established integer-offset/in-place contract is retained.

Release MCP also verifies default/explicit-all-source byte parity, invalid
recipe/alias/scalar refusals without edits, exact staged acceptance and
single-use tickets, selection staleness and discard, opaque new-layer equivalence,
native Patch, saved native reopen, original artwork editability, snapshots and
owned-process crash recovery retaining fill/artwork/Undo.

Cancellation timing starts only after observing native stage 3 and positive
progress, rather than before scheduling. The final byte case observed progress
0.1993608 and drained in 61.1 ms; the typed case observed 0.1865259 and drained in
63.9 ms. Both left zero scratch bytes. Ready candidates increased retained payload
from 0 to 2,124,800 bytes (byte) and 2,126,976 to 5,315,264 bytes (typed); stale Apply
released the candidate back to the exact respective baseline. The same probe
checks ordinary background-job cancellation and that a synchronous request while
the worker is occupied preserves the previous preview ticket. All launched
processes use dummy credentials and private stores and were confirmed exited.

The final Release executable SHA-256 is
`5ad0d5417a0d721843c207f11e173d13cb8bec6b935f5971a1238d65be5bb76d`.
The actual-source UI harness links the same production core archive, SHA-256
`4ba4b4f9eeea8b8e24e8f42c090a153e8fcc115b36c081cd260e539ba7e7f8c5`.
Its separate probe executable is built from copied app sources plus probe entry
points; it does not pretend to be the production app binary.

Reproducible commands:

```sh
CARGO_TARGET_DIR=/private/tmp/rz-phase9-tests CARGO_PROFILE_RELEASE_LTO=false CARGO_BUILD_JOBS=4 RZ_RETOUCH_PHOTOGRAPHS=/private/tmp/rz-phase9c-photographs make test
CARGO_TARGET_DIR=/private/tmp/rz-phase9-tests CARGO_PROFILE_RELEASE_LTO=false CARGO_BUILD_JOBS=4 make lint
cd core && cargo fmt -- --check
make typecheck
make app
python3 scripts/verify_phase9e.py --report /private/tmp/rz-phase9e-mcp-final.json --require-current
python3 scripts/verify_phase9e_lifecycle.py --report /private/tmp/rz-phase9e-lifecycle-final.json --require-current
python3 scripts/verify_phase9e_ui.py --report /private/tmp/rz-phase9e-ui-final-qualified.json --require-current
```

The `cd core` formatting command runs separately; subsequent `make` and probe
commands run at the repository root. Runtime tests require an AppKit session and
localhost MCP. Photographs are the pinned external inputs above. Reports live at
the listed paths for this run; full logs are `/private/tmp/rz-phase9e-full-tests.log`,
`rz-phase9e-lint.log`, `rz-phase9e-typecheck-qualified.log` and
`rz-phase9e-build-qualified.log` in the same directory. The live catalog receipt is
`/private/tmp/rz-phase9e-catalog.json`.

## Corrections found during qualification

- Review found that stale ticket acceptance retained a ready candidate, and that
  a synchronous busy request could replace the prior ticket before refusing.
  Stale acceptance now releases it; the busy guard now runs before replacement.
  Final lifecycle probes measure both behaviors and payload return to baseline.
- Real sheet events exposed an invalid dismissal path and an over-constrained
  source view width (930 pixels inside 926 available). The sheet now dismisses
  through its registered presenter, makes repeated/owner-close cancellation
  idempotent and sizes its source view from the shared insets. Final tests exercise
  Apply detachment and owner close in all four CAF cases.
- The first MCP Patch fixture selected only the dark blemish, pinning its membrane
  contour to that corruption. The corrected fixture surrounds the same blemish
  with a clean boundary; the brightness gate is unchanged. This was a fixture
  precondition correction, not a synthesis quality improvement.
- Early UI harness replay omitted the owned document ID and selected the empty
  untitled document; its scalar check also bypassed the editor's sole target
  setter. The harness now routes both through the actual public context. AppKit
  event pumping follows the established selection probes. Final results above
  come from corrected, unchanged-input runs, not those failed preliminary runs.
