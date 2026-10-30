# Phase 9 — editable photographic filters and retouching

## Current slice

**9A–9H are complete within their qualified scope.** Phase 9D verifies
P7-15–P7-17 through native sampling, transformed source overlays, explicit storage
expansion and native spot-heal/red-eye. Phase 9E verifies P7-18 through constrained
native content-aware fill, exact candidate comparison, output choices and Patch.
[Phase 9F](phase-9f-selected-tone-status.md#qualification) verifies P7-06/P7-07:
selected native tone/spatial filters, fine parameter entry, separate Auto analysis
scope and exact weighted composite/raw-layer/backdrop statistics. [Phase 9G](phase-9g-selections-masks-status.md#qualification) verifies P7-08–P7-11 through native scalar editing/display, source selections, authoritative selection/Quick Mask coverage and typed auxiliary mattes. [Phase 9H](phase-9h-paint-fill-status.md#bounds-and-qualification) verifies P7-13/P7-14 through native pressure/timing, paint/fill parity and exact canonical replay. **Eighteen Phase 7 assignments are verified and 18 remain open.** Phase 9I retains the original P7-12 cutout quality gate; the whole Phase 9 remains open.

The [9I continuation](phase-9i-cutout-status.md) records five frozen candidate evaluations and internal native cutout assembly. None passes the original combined alpha/color gate; advanced brushing and decontamination remain disabled. The assembly preserves native foreground source alpha and a separate refinement mask. The [execution continuation](phase-9i-execution-status.md) adds controlled byte/native preparation without qualifying an estimator.

The [next-step plan and restart handoff](phase-9i-next-steps.md) replaces an
open-ended request to finish 9I with a bounded contract/evidence/feasibility
assignment. Assisted results remain separate from the original automatic gate;
independent Phase 10 work need not wait for this research.

The [feasibility prerequisite review](phase-9i-feasibility-status.md) now defines
the separate assisted protocol and conditionally selects RVM, with checkpoint
terms and genuine guided-input readiness still unresolved. No new candidate was
scored and no Phase 9 gate changed. Independent PSD import work can proceed.

This file is the shared progress record. 9A starts from committed Phase 8G plus
the subsequent public tool-catalog documentation; 9B starts from committed 9A;
9C starts from committed 9B; 9D-1 starts from committed 9C.
The [implementation plan](professional-photo-editor-implementation-plan.md#9-editable-photographic-filters-and-retouching)
remains the scope authority. Root owns the final audit and commit.

## Agreed source contract

An ordered **filter stack** belongs to a retained **layer binding**, preserving the
original source master and existing RAW/local-mask or host-media resource
attachments. Evaluation is source-profile conversion into the document's working
profile → the stack, first entry to last, in source coordinates → accumulated
layer affine and fallback-depth conversion → independent layer mask, styles and
compositing. A display preview or previously filtered fallback never becomes a
filter input. Phase 9A carried exactly one Gaussian; Phase 9B widened the same
contract to several entries without changing any of its ordering guarantees; and
Phase 9C widens it again to operators that bring their OWN neighborhood instead
of sharing the Gaussian, still without changing them.

This slice exposes parameter editing, enable/disable, reordering, insertion and
removal, per-entry opacity and blend mode, an independent native coverage mask
per entry, a bounded preset library and explicit removal of one filter or the
whole stack. Native saves, snapshots and recovery retain
both editable state and usable rendered fallback; reopening recalls parameters
without requiring original external URLs or camera/movie decoding. Unknown future
filter semantics preserve the cached rendering and refuse implicit reinterpretation.

All edits use immutable source/state captures, Phase 4 admission and the existing
single worker, native cancellation, stale-result checks and one central Undo
publication. Preview reuse is bounded and its identity includes source revision,
profile and every relevant filter parameter; changed mask or presentation state
must invalidate only the stages it affects. UI, MCP and Actions share the same
captured edit contract.

## Implemented contract

The integration includes **Filters > Live Filters**, **Filters > Filter
Presets**, retained-layer context actions and explicit removal. The dialog owns
one captured source and coalesces edits through one Phase 4 worker. A stack list
adds, removes and reorders entries; the parameter column is rebuilt for whichever
entry is selected, so one control identity never serves two filters and a canvas
gesture can only ever reach the selected one. Fit and native 100% previews support
Before/After/Split, pan and an effective coverage overlay for the selected filter.
Radial/linear gestures replace source-coordinate coverage; add/subtract brush
gestures edit it. Numeric controls retain binary64 radius, amount, threshold,
opacity and mask properties. Apply publishes the reviewed immutable result;
Cancel, owner close and stale captures cannot publish. Modal teardown clears
relevant cache generations and drains any cancelled worker.

Phase 9C adds no tool of its own and no control on the existing ones beyond the
three `reduce_noise` settings: the sheet's Add menu, the operator enum, the
descriptor's legal keys and the recorded step all read ONE per-operator table
(`LiveFilterOp.parameters`), so a control cannot be accepted in one place and
silently dropped from a recorded step or a saved preset in another — which is
the shape of defect that produces a replay with a different picture and no
error. A control sent to an operator that does not own it is refused by name,
naming the operators that do own it.

The public tools are `get_live_filter`, `set_live_filter`,
`set_live_filter_stack`, `add_live_filter`, `reorder_live_filters`,
`remove_live_filter` and the four `*_live_filter_preset` tools. `set_live_filter`
keeps its Phase 9A meaning — patch one filter, creating the stack when none
exists — and its `filter_index` stays optional while the layer has at most one
filter, so recorded 9A steps replay unchanged. A modal session records one
canonical `set_live_filter_stack` carrying the resolved filters plus, per edited
filter, its starting mask hash and the source dimensions for coverage replay.
Byte and typed stack inspection use the same compact filter summary and cheap
presence under denied metadata admission. Profile conversion uses checked
filter-aware work; assigning a profile to a retained source still requires
explicit rasterization first.

A stack holds 1–16 ordered entries over five operators. An operator is a
NEIGHBORHOOD plus a COMBINE step, and `live_filter_kernels` dispatches both from
one `Neighborhood` value, so a cached plane and its cache key cannot drift
apart. Three operators share the Gaussian neighborhood: `gaussian_blur`,
`unsharp_mask` (amount 0–5 as a multiplier where 1 is 100%, threshold 0–1 in
normalized contrast where 1/255 is one 8-bit level) and `high_pass`. Radius is
binary64, 0–64 untransformed source pixels for every operator; the Gaussian
support is `ceil(3 × sigma)`, using normalized weights and clamped source edges.
Color/alpha are premultiplied for convolution in the encoded working profile.
Gaussian Blur replaces the sample including alpha; every other operator writes
COLOUR ONLY — alpha passes through exactly and a sample whose alpha is zero is
left exactly alone, so hidden colour can never be brought into view — and the
sharpening pair clamps to [0,1] before blending, because both overshoot. High
Pass returns the detail around the neutral 0.5 pivot, so a constant plane at any
radius returns the pivot and nothing else.

Phase 9C adds the two noise-cleanup operators the plan names, and they are
SEPARATELY EVALUATED — neither shares the Gaussian.

`reduce_noise` is an alpha-weighted guided filter (the He/Sun/Tang local linear
model) over a Rec. 709 opponent decomposition. Its cost is INDEPENDENT of
radius, because every window statistic is a box sum computed as two 1-D sliding
passes; that is the only reason a 25-pixel chroma window is admissible at full
photographic resolution, and it is why a convolution-family or bilateral kernel
was rejected rather than the other way round. `sigma` sets the analysis windows
— `ceil(3 × sigma)` for luminance and four times that, capped at 256, for
colour. The two halves are deliberately asymmetric, and the asymmetry is the
design: `luminance` (0–0.25) is the noise LEVEL to remove in normalized encoded
units, so it decides what COUNTS as grain, because luminance detail is real
image content; `color` (0–1) is simply the AMOUNT of chroma smoothing, because
chroma detail almost never is. `detail` (0–1) is the fraction of removed
luminance detail added back. The cached plane is a DELTA in opponent space, not
a smoothed sample, which is what makes an inactive half a bitwise identity
rather than a correction that cancels to within a rounding, and what makes a
chroma-only correction leave luminance exactly unchanged. Statistic planes are
binary64: the local variance is `S2/S0 − m·m`, and at a small noise level that
difference cancels far below f32's resolution. A colour boundary carrying no
luminance step is invisible to the chroma guide and will bleed — inherent to
every luma-guided chroma denoiser, and what `color` trades against.

`dust_and_scratches` is a straight per-channel LOWER median over the lattice
disc `dx² + dy² ≤ fl(radius²)`, where `fl` is one binary64 round-to-nearest
product; round-to-nearest is monotone, so the support can never shrink as the
radius grows, and it grows in visible steps as the radius crosses each ring of
pixel centres. A disc rather than a square: a square median reaches `√2·r`
diagonally and would erase diagonal features 41% larger, a directional bias on
exactly the fine structure this operator exists to preserve. Alpha decides
MEMBERSHIP rather than weight — a rank statistic cannot be weighted without
becoming a different operator — so a transparent neighbour never votes and a
neighbour at one code of alpha votes as loudly as an opaque one. Clamped
duplicates each vote separately, or the support count would become
position-dependent. `threshold` (0–1, shared with `unsharp_mask`) gates the
replacement per channel with a strict `>`, which is exactly what makes threshold
0 a plain median bit for bit rather than a bypass. It is ONE operator, not a
`median` beside a `dust_and_scratches`: two would be two cache identities
holding an identical rank plane, and the commonest retouching gesture — lay down
a median, raise the threshold until fabric texture returns — would cross that
boundary and recompute the most expensive plane in the stack.

Each entry mixes its own output over its own input: the blend function sees the
filter's input as backdrop, weighted by that backdrop's alpha exactly as the
compositing formula's `(1-ab)·cs + ab·B(cb,cs)` term, and mask coverage times
opacity then drives the existing precise alpha gate. Normal skips the blend table
entirely, which is what reproduces the Phase 9A arithmetic byte for byte. Every
layer blend mode is available except Dissolve, whose dither needs canvas-absolute
coordinates a source-stage filter does not have, and Pass Through, which belongs
to a group; both refuse.

The processing depth is the greater of source and document depth: source ICC
conversion quantizes at that native depth, and the WHOLE chain then runs in one
straight f32 plane with a single processing-depth quantization after the last
active filter — never one per stage. Radius zero, opacity zero and a disabled entry
bypass any entry exactly; so do a zero Unsharp amount, a `dust_and_scratches`
radius below one pixel (its disc then holds the centre alone) and a
`reduce_noise` with both halves off — `luminance` 0 or `detail` 1, together with
`color` 0. High Pass declares no bypass beyond the shared three, so a control
dragged to zero can never replace the picture with its flat pivot. These are
ENTRY-level bypasses: the stage is skipped entirely, which matters because an
operator that merely computes an identity output still squares the colour under
Multiply and is not bit-exact below full weight. An
entry whose operator or evaluator this build does not recognize makes the WHOLE
stack opaque rather than half-evaluating it. The existing profile, affine and
fallback evaluation still applies. No canvas growth or cumulative use of an
already filtered fallback occurs.

Per-kernel work refusal is not enough once a stack runs several, so the forecast
sums every active entry's kernel, mask and per-pixel cost against the same
two-billion work-unit cap and refuses before allocation. The destructive twins
run once rather than inside a coalescing preview, so they declare a separate
twelve-times budget instead of refusing an ordinary photographic radius on a
full-size layer.

Each entry owns an independent native u16 coverage mask. Density 1 uses the mask;
density 0 or a disabled mask reveals the entire filter. This is the existing
layer-mask policy, distinct from RAW-local contribution density. Mask feather is
0–256 source pixels using the shared padded coverage kernel. A missing mask means
full coverage. **Coverage follows its filter's identity**, so reordering,
inserting and removing filters all move the right plane, and a caller that wants
a filter to start at full coverage gives it a fresh identity rather than asking
for a second alignment rule; masks cross the FFI as an array positional with the
descriptor's filters. Requests admit at most 64 mask commands, 4,096 aggregate
brush dabs and 256 KiB of JSON, counted across every filter in one request; the
native brush has its separate 256-million evaluated-pixel work cap. Complete source evaluation has a separate two-billion work-unit cap;
12 MP at sigma 6 already exceeds it and is refused before allocation. Coverage
remains outside recipe/descriptor JSON. Each filter mask retains the existing
64-million-source-sample plane cap; RAW, generic resources and filter masks share
the 512 MiB native binary pool. Larger supported workloads are a 13D gate, not
inferred from the allowed sigma range.

Native byte version 23 and typed version 24 persist binding-owned filter state and
mask references through the existing deduplicated binary resource pool, unchanged
by 9B and unchanged again by 9C: the LFLT record always carried one descriptor
plus one mask reference PER FILTER, so a wider stack is spent reserved room
rather than a format change, and no native document version moves. Adding an
OPERATOR needs no version move either, and a schema bump would be strictly
worse: an older build already meets an unknown operator through the
opaque-stack branch, whereas a bump would make even an all-Gaussian stack opaque
to it the moment 9C re-edited it, since every canonical write normalizes to the
current version. What that forward compatibility does require is that a control
never appear on an operator that does not own it: `LiveFilter` is one struct
with `deny_unknown_fields`, so such an entry would still be a KNOWN operator to
an older reader, and serde would refuse the whole DOCUMENT rather than leaving
the stack opaque. `validate` therefore checks control ownership in both
directions, per operator, and a test pins that a foreign control is refused
rather than written. The descriptor's own `schema_version` becomes 2
on any edit; a Phase 9A reader meets that through its existing future-version
branch and preserves the cached rendering instead of refusing the file, which a
version bump would have forced. Schema 1 still reads, and shrinking a stack
shrinks its mask references so the writer cannot produce a file its own reader
would reject. Files without live filters keep their established writers. Original source masters,
RAW/local attachments and encoded host resources remain distinct. Same-size
source replacement preserves filter coverage; a geometry-changing replacement
with a filter mask refuses until that mask is removed. Unknown filter semantics
preserve cached open/resave and refuse semantic rerender; explicit removal remains
available. Larger geometry semantics belong to 12F.

Three cache stages share a 128 MiB bound per retained binding, including retained
mask, descriptor and ICC ownership, and are shaped around the fact that only the
last active filter is edited interactively: the fully gated result of everything
before it, the unmasked premultiplied convolution of that plane at its radius,
and the masked native source pixels. Keys are exact, never hashes — source
identity and revision, target ICC bytes, processing depth, the canonical JSON of
the entries already applied and their coverage by pointer. The kernel key is the
operator, the binary64 radius and any further parameter that changes that plane
- today only `reduce_noise`'s `luminance` and its half-presence bits - so
amount, threshold, detail, colour, opacity, blend mode and mask edits all reuse
the expensive pass; a radius change reuses the prefix; an affine change reuses
the masked stage; changing an earlier filter invalidates all three. The operator
stays IN the key even where two operators would compute the same plane, so a
ranked disc can never satisfy a convolution lookup at the same radius. The two
commonest cleanup drags are therefore pure kernel-stage hits: a dust threshold
(the rank plane is a function of the input, the disc and the alpha-membership
rule and nothing else) and Preserve Details or Colour Noise (pure
post-multipliers on the cached correction). A single-filter stack retains no prefix plane, so Phase
9A's two-entry memory profile is unchanged. The convolution keeps retention
priority and the masked stage is evicted first. Headroom-aware admission keeps
caches only when the full live set and requested peak fit, otherwise evicts and
remeasures before refusing. Cache generations reject late insertion after clear.
These are bounded working estimates, not an operating-system memory ceiling.

The destructive twins are **Filters > Unsharp Mask…**, **High Pass…**, **Reduce
Noise…** and **Dust & Scratches…**, plus `apply_filter`'s `unsharp_mask`,
`high_pass`, `reduce_noise` and `dust_and_scratches` kinds. All four are thin
wrappers over ONE shared body in `ops_photographic.rs` (renamed from
`ops_sharpen.rs`) that routes through the same neighborhood dispatcher and
combine step as the live stack, so a destructive pass and the same live filter at
full coverage, Normal, opacity 1 agree within one byte code — and Dust &
Scratches at threshold 0 agrees EXACTLY, because a rank filter selects and
introduces no rounding of its own. That shared body never calls `mix`, which is
what the agreement claim rests on. 9C's twins are an ELECTIVE addition: unlike
9B's sentence, the 9C plan sentence says nothing about twins. They ship because
the gap review tracks the destructive filter menu as its own row, because the
exact threshold-0 agreement is a strong cross-check the live path alone cannot
give, and because the interactive ceilings below make the destructive path the
answer for full-size files. They
reach the existing selection-scoped destructive path unchanged, so the write is
confined to the active selection's coverage while the neighborhood still samples
the whole target. Their sheets show amount in percent and threshold in 8-bit
levels, and record the normalized values the core takes.
Repeat Last Filter and Actions need no new registration: both ride the existing
`apply_filter` tool, and the 16-bit refusal is the existing `apply_filter` rule
rather than a new entry. They are 8-bit only, exactly like every other
neighborhood filter here; typed destructive spatial filtering remains 9F/P7-06.

**The interactive ceilings, stated plainly rather than left to be met as a
regression.** Every published figure is a `retained_source_render::work_plan`
total — `live_filter_evaluate::plan` is not the last word, because the render
gate adds the affine sampling and a full source colour visit on top. One entry,
Normal, no mask, at 1:1 placement: `reduce_noise` with both halves admits about
11.5 MP (a single half more), and `dust_and_scratches` about 11.7 MP at radius 1,
5.5 MP at radius 2, 1.6 MP at radius 4 and 0.42 MP at radius 8 — the rank cost
grows with the disc area, while the denoise cost does not grow with radius at
all. A placed layer costs more: bilinear resampling removes roughly a tenth and
Lanczos3 roughly half. Against the twelve-times destructive ceiling,
`reduce_noise` never refuses on a legal image at any radius, and the dust twin
admits about 71 MP at radius 2 and 5 MP at radius 8. So interactive cleanup
covers an ordinary photograph at photographic radii, and the destructive twins
are the answer for full-size files. Larger interactive workloads stay a
13A/13C/13D gate, exactly as the 9A/9B record already says for 12 MP at
sigma 6. `reduce_noise` also carries a MEMORY forecast that can bind before
its work forecast on a large typed layer: its binary64 statistic planes are
about 80 bytes per pixel at the peak of the wider half, against the
Gaussian's 32.

A filter preset is a bounded versioned library entry built on the same contract
as the RAW preset library — `RZ_FILTER_PRESETS_DIR`, digest filenames, an
advisory cross-instance lock, atomic publication, whole-library strictness, 128
presets of at most 8 KiB. It stores settings, order, opacity, blend modes and
mask properties and NEVER coverage, which belongs to the source it was drawn on.
Applying one adopts the target's filter identities slot for slot, so coverage
already drawn stays in place; `keep_masks: false` takes fresh identities and
therefore full coverage. Library writes are preferences: outside document Undo
and never Action steps, and applying records the resolved `set_live_filter_stack`
so a later rename or edit cannot change replay.

## Frozen independent gates

Before observing product output, qualification fixed a direct f64 Gaussian oracle
at **3 u16 codes** per component, or **1 byte code** at byte processing depth.
Profile conversion plus Gaussian has a separate **4-code u16** bound; adding the
existing bilinear affine uses **6 codes**. Phase 9B reuses those same figures for
one operator and fixes, before observing output, **6 u16 codes** for a chain of
up to three operators — the same escalation the Gaussian slice used for profile
conversion and affine — and **1 byte code** between a destructive twin and its
live filter at full coverage. The 9B oracles are a direct 2-D convolution, an
independently re-derived coverage lift, independently written W3C blend functions
and a hand-derived alpha gate, none of which call the production sampler, kernels,
blend table or gate; the chain oracle runs entirely in f64 with a single terminal
quantization, so the reference shares no approximation with the thing it checks.
That structural independence was previously written up here as making an
intermediate rounding in the product *detectable*, which the 9B review measured
and disproved: a full per-stage quantization moves these fixtures by at most 2
codes against the frozen 6-code chain bound, so the tolerance absorbs it. The
single-quantization rule is held by construction and review, not by this
tolerance; the claim is corrected here and in both test headers.
Phase 9C fixes its own bounds before observing any product output, and escalates
NOTHING. `reduce_noise` alone reuses the same **3 u16 codes** and **1 byte code**
one-operator figures: its output sensitivity to the cached plane is `1 − detail`
and `color`, both at most 1, the plane stores a DELTA whose magnitude is the
noise itself, and every window statistic accumulates in binary64, so the
operator's own contribution is far below the shared terminal rounding that
actually spends the budget. `dust_and_scratches` alone, as the first entry at
full coverage, Normal, opacity 1 and matching profile bytes, is pinned by
**exact equality** in both depths — a rank filter SELECTS, so there is nothing
left to round, and the exact gate is the only one that catches an implementation
which quietly averages two middle elements. Below full weight the output is a
weighted average of two f32 samples and the ordinary 3-code bound applies. A
chain of up to four operators keeps the 9B **6-code** figure.

Two bounds are declared rather than invented, because a rank filter is exact but
DISCONTINUOUS in its inputs. Where two candidates inside one window lie within
the chain's accumulated f32 error, the product and the oracle can select
different neighbours and the outputs then differ by the whole gap between them —
which is not a code tolerance at all. So a chain case containing a rank filter
carries the 6-code bound only on a fixture whose intra-window separation the test
itself asserts (≥ 64 u16 codes), and a near-tied fixture is pinned by MEMBERSHIP
alone: every output sample, in every channel, is either the input sample or a
value some alpha-positive neighbour actually had. That membership gate is
tolerance-free, reference-free, and impossible for a convolution-family oracle to
pass by accident.

The 9C oracles are as structurally independent as the 9B ones. The rank
reference rebuilds its support per pixel by scanning a deliberately OVER-LARGE
square and letting only the disc predicate reject it, so an off-by-one in the
product's precomputed offset list is visible, and it fully sorts where the
product partially selects. The guided reference sums each window DIRECTLY where
the product slides two 1-D passes, and writes the Rec. 709 weights as its own
binary64 decimal literals rather than widening the product's f32 constants. The
colour-only alpha law is re-derived positively on the oracle side rather than
copied. The exact-oracle band for the guided reference is a radius whose chroma
window leaves genuine interior positions on the fixture; outside it the direct
reference would stop checking the sliding re-seed, so the wider radii are pinned
by structural gates instead — an alpha-weighted box sum over a constant plane
must reproduce that constant exactly at every pixel and every radius, and an
inactive half must be a bitwise identity.

Ordering significance, disabled-equals-absent, per-filter mask
independence and every declared bypass are exact checks. Alpha participates in these comparisons.
Near-opacity mask cancellation, tiny filtered alpha and support boundaries just
below/on/above sigma 1/3, 1 and 8/3 are explicit adversarial cases. Exact bypasses,
outside-mask samples, original master/ICC, Undo/Redo, A→B→A and persistence are
exact checks, not applications of the numerical tolerance.

The completed gates cover independent full-depth sample and native parser checks,
source/profile/geometry ordering, parameter recall after reopen, RAW/local and
host-frame preservation, actual UI plus external MCP, canonical Actions,
snapshot/recovery, refusal/stale/no-op/resource/cache checks and source/artifact
provenance. The final target-15 archive, formatting, all-target Clippy, full Rust
suite, authoritative Swift typecheck and Release build all pass. Each passing
gate has its own recorded evidence; later filter operators remain unqualified.

## Corrections the 9B review found

An adversarial review of the slice (five reading dimensions, per-finding
refutation) confirmed ten defects in the first implementation. They are fixed
here, and each one is named because the reasoning generalizes:

- **A colour-only operator must keep a transparent sample's colour at every
  weight.** `evaluated` returned such a sample verbatim, but the precise alpha
  gate then saw zero alpha on both sides, had no premultiplied colour to mix
  and wrote `[0,0,0,0]` — at every weight except exactly 1, where its shortcut
  hid the problem. Full coverage is therefore the one case that could not
  detect it, and it was the only case under test. `mix` now returns early for a
  colour-only operator on a transparent sample, the oracle encodes the same
  law, and `colour_only_operators_keep_transparent_colour_at_every_partial_weight`
  drives filter opacity and a partial mask. This is also what makes the live
  filter and its destructive twin agree below full coverage.
- **A "fresh" filter id has to be fresh against the SAVED stack.** Coverage is
  resolved by id against what the layer has saved, so minting against an
  unpublished in-memory stack (the sheet after a Remove, or
  `apply_live_filter_preset` with `keep_masks: false`) handed a new filter the
  removed one's plane. `availableID` and `adopting` now take the saved
  identities as reserved.
- **Phase 9A descriptors were unreadable.** Swift's synthesized `Decodable`
  ignores property defaults and demands every non-optional key, so a
  `schema_version` 1 entry — written before `opacity` and `blend_mode` existed
  — threw `keyNotFound`. `LiveFilterEntry` now has an explicit `init(from:)`
  mirroring the Rust `#[serde(default)]` on exactly those two fields.
- **A recorded step must be replayable.** A modal session could record a
  `mask_edits` entry for a filter it had removed; `evaluate` skips an orphan but
  `set_live_filter_stack` refuses one by name. The session drops a removed
  filter's pending gestures and `publish` records only edits the applied stack
  still has a filter for.
- Mask-command bounds are again checked before any plane work and aggregate
  across the whole request, and the render uses the owned copies `capture`
  returns. The destructive twins consult the work ceiling before allocating
  their float plane rather than after. `add_live_filter` refuses `amount`/
  `threshold` on a non-sharpening operator instead of dropping them. The three
  mutating preset tools joined `Action.notAStepTool` beside their RAW twins.
  Reordering in the sheet clears a stale input error, since the same call
  rebuilds the fields the error refers to.

One residual gap is recorded rather than closed. The claim that Normal
reproduces the Phase 9A arithmetic byte for byte rests on `blend_of` returning
`None`, and no black-box test can distinguish that from a blend-table entry that
happens to agree: a descriptor written without `blend_mode` is canonicalized to
Normal before evaluation, so both candidate inputs take the same path. The
equivalence was instead confirmed by reading the two implementations against
each other — Phase 9A (`c725d88:core/src/live_filter_evaluate.rs`) called
`gate_source_precise(&mut value, straight(blurred), weight)` with no blend step,
which is what `mix(..., blend: None, ...)` does here — and the arithmetic itself
stays pinned by the 9A numeric suite.

## Remaining owners

Phase 9D supplies the native PatchMatch foundation and spot-heal integration;
9E completes content-aware fill, source inclusion/exclusion editing, destination
choice and meaningful final-result comparison within the bounded qualification
below. The two confirmed 9D-1 review
findings are corrected by the admitted source wrappers and captured native
source/ghost path described below.

General selected destructive tone/filter and analysis scope are qualified
separately in 9F; selection/Quick Mask precision and scalar editing are qualified
separately in 9G. Advanced geometry is
12F and broader image-size/hardware/latency qualification is 13A/13C/13D. The
existing 8F expanded ProRes decoder-conformance limitation stays with 11D/13D.

## Qualification

### 9E — source-constrained content-aware fill

The [9E receipt](phase-9e-content-aware-status.md) records the source-mask,
selection, output and exact-candidate contract and verifies P7-18. Both depths
share the existing PatchMatch and membrane implementation. Native source,
pyramid, vote and solve values remain f32 until the final u16 writeback.
Versioned ordered inclusion/exclusion brushes and rectangles are independent
of boundary context and propagate source eligibility conservatively through
every pyramid level. Legacy byte defaults remain exact. Output can replace the
target or create a layer while preserving the original pixels and description.

The actual sheet prepares one captured candidate; Before/After and Fit/100%/pan
inspect it, and Apply publishes that same owner once. Source or parameter changes
invalidate it. MCP adds preview, single-use acceptance and explicit discard with
the same stale-state and owner-lifetime checks. Actions retain resolved source
geometry and output, never preview tickets. Native Patch uses the captured
retouch-heal path and preserves its rounded integer offsets and in-place-only
output; byte Patch retains its established foreground solver and byte overlay.

Final checks pass: **1,299 Rust tests, two existing ignored; all-target Clippy;
formatting; authoritative Swift typecheck; production Release build; 16 owned
MCP workflow groups; eight cancellation/lifetime groups; and 12 actual UI groups**.
The UI groups cover four content-aware sheet/output cases and eight Patch
depth/direction/polygon-or-selection cases. Native reopen, Actions, snapshots,
and owned-process crash recovery retain the committed result and original artwork.
At 9E qualification, the public reference matched the owned final app's
**180-tool catalog**; 9G subsequently increased the current catalog to 181.

Four independently measured photographic repairs pass the fixed small-hole gates:
MAE 8.76–9.64 byte-code equivalents, with 8.23–12.04% of corruption error remaining.
Different excluded contaminants produce exactly the same repair. Observed native
stage-3 cancellation drains scratch to zero in 61.1 ms at byte depth and 63.9 ms
at native depth. Candidate payloads return to their pre-preview baseline after
stale rejection. The receipt gives fixed fixture hashes, bounds, source hashes,
commands and report paths.

This closes the bounded workflow, not arbitrary large-hole photographic quality.
The documented 24×24 periodic-hole phase ambiguity remains. Masks, scalar planes,
saved channels and Quick Mask refuse this RGB operation; general selection/scalar
precision is qualified separately in 9G. Partial-alpha new-layer output retains the established
retouch compositing behavior, and gated enclosing groups refuse new output.

### 9D — native and transformed retouch

The two earlier 9D-1 findings are fixed. Byte source reads now obtain a host
memory admission and a real core job token, including Current Layer's raw copy.
Typed gestures capture a native plate; source transformation happens in native
precision before a separate display reduction. Neither the drag nor hover source
overlay falls back to `canvas.image`. Source scale, rotation and horizontal/vertical
flips share the recorded inverse canvas matrix; Aligned strokes retain that matrix.

Both depths expose Current, Current and Below and All Layers, ignoring adjustment
entries above the target, and output to a new Retouch layer. The typed compositor
cut keeps the explicit group kind, visibility of uncropped contributors and ancestor
gates. A current-only `source_layer` names an independent raster; contradictory
mode/source controls refuse. Legacy `sample_all_layers` meanings and per-depth
absent defaults remain unchanged.

Source coordinates are continuous canvas coordinates at pixel centres. The
`source_transform` array is `[a,b,c,d,tx,ty]` mapping destination centres to source
centres. Native byte/u16 paths use the same premultiplied bilinear taps with
transparent out-of-bounds samples; hidden RGB never enters a visible neighbour.
Healing uses connected components and the existing membrane solver, preserving
destination alpha, including a single native alpha code. Every untouched storage
sample remains exact. Explicit `expand_destination` unions raster storage with the
canvas, retaining off-canvas originals and independent mask placement. Clone can
paint newly allocated transparent storage; heal/spot preserve its zero alpha.
For a repair over underlying content, New Retouch Layer heals a captured
current-and-below backdrop and leaves the original layer untouched. As in 9D-1,
putting a repair at a translucent backdrop's own alpha can increase composite
alpha; this is not an alpha-replacement operation. New output inside a gated
group refuses rather than applying the group's mask/opacity twice.

Native spot healing shares the shipped PatchMatch search, pyramid, vote and
Poisson writeback. The [synthesis qualification receipt](phase-9d-synthesis-qualification.md) records the exact fixture gates and measured limits. Its specialization carries RGB as f32 native-code values
through the whole solve and quantizes once into u16 destination storage. Byte
specialization retains its historic integer rounding and search arithmetic.
Native red-eye detection uses continuous RGB through the same red-dominance,
saturation/hue and pupil-component rules; coverage feathering is scalar only.
Correction retains original alpha and supports the existing manual and automatic
UI/MCP commands. Automatic face location may use a display image, while every
corrected RGB sample comes from the captured native document.

Spot/heal requests carry conservative footprint bounds before host input allocation;
runtime coverage outside those bounds refuses. Exact component/ring planning keeps
the shipped memory/work caps. Nested sampling/render stages retain a reservation
floor for all compound-operation buffers. Typed spot jobs validate document revision,
immutable source, active/selected layers, paint target, Quick Mask and selection
before one central Undo/Actions publication. There is no native document version
change: only existing native pixel/layer records are written.

The actual gesture probes found and fixed three additional defects. Byte spot
healing copied its borrowed overlay after a display refresh had already cleared
it at mouse-up; the copy now precedes refresh. Byte clone/heal draw dabs while
legacy MCP defaults draw a continuous path, so new gesture recordings explicitly
carry `footprint: "dabs"`; absent arguments retain old Action behavior. Finally,
the shared job publisher cleared the original target's artwork description even
for new-layer output. An explicit target-rasterization policy now preserves the
original description/master for read-only sampling into a new repair layer.

| Gate | Result |
| --- | --- |
| Independent transformed-source/alpha/group/expansion/healing FFI | Seven groups passed; four f64 bilinear/source-over source matrices, three distinct sampling modes, below-adjustment oracle, mask/offset/native reopen, harmonic healing and exact-copy/profile-conversion commutation. |
| Native red-eye | Four groups passed: f64 neutralize/darken with fractional selection, alpha1/catchlight preservation, component/outside-rectangle no-ops, strict arguments and active cancellation. |
| Native spot synthesis | Seven groups passed in the dedicated photographic invocation; source texture, native alpha/write scope, seed reproduction, capacity/bounds and active cancellation. |
| Photographic spot quality | Pinned astronaut/fur: output mean RGB error 10.804/9.129 byte-equivalent codes, 13.489%/8.358% of the injected corruption. Both unchanged gates are below 15 codes and below 25% of initial error. Source photographs are byte captures with a disclosed synthetic sub-byte carrier, not camera-native 16-bit captures. |
| Synthesis ambiguity | The exact-reconstruction gate for a 24px periodic hole failed: unknown texture phase can shift by the full source contrast (1, 91 or 20,000 native codes). This failure remains recorded in the test. The separate 8px-hole exact-reference gate passes; larger-hole qualification claims source range and survival of native detail, not unique texture reconstruction. |
| Formatting / all-target Clippy | Passed with `-D warnings`; final `cargo fmt` applied. |
| Authoritative Swift typecheck | Final run passed with zero errors on the complete source, including catalog descriptions and the source-control label fix. |
| Standalone source/transform/expansion FFI | New null-safety sweep, byte/native transparent-edge reference, cancellation and off-canvas storage tests included in final full regression. |
| Actual AppKit UI | Ten groups passed at backing scale 1×, using source-identical app code and final core archive: real source-choice/hover/drag/More controls, clone/heal/spot/red-eye in both depths, exact recorded Action replay over 12,288 RGBA components per case, Undo/Redo, and both-depth editable-artwork new-layer preservation. 373 source files/370 compiled inputs unchanged, all owned documents/process exited. Final More labels are fully readable; before/after screenshots verify the width correction. |
| Production Release | Normal-LTO `make app` passed. Final executable SHA-256 `a70246e8cbc64ce40c8a5e202975d6e1e337769ede2feb30aaa053733d5ef562`. |
| External MCP / persistence | Existing seven-group regression plus twenty extended groups passed on the final app; 751/752 captured source inputs unchanged and owned instances exited. Both depths cover numeric source mapping, Actions/Undo, selected write scope, grading/new output, heal/spot/native reopen, expansion, red-eye, original artwork preservation, saved snapshot and actual owned-process crash/recovery. Recovered composites are exact, artwork remains editable, recovered Undo is exact and recovery is dirty. |
| Live MCP reference | Regenerated and compared to the final 178-tool catalog; new source matrix/expansion/footprint arguments and typed capability descriptions agree. |
| Compound progress | A final production-archive C probe observed actual inpainting stage 3 at 0.201035 (All/in-place) and 0.200647 (Below/new-layer), followed by atomic cancellation and zero scratch. Source renders no longer report the whole retouch finished before its solve begins. |
| Full Rust regression | **1,290 passed, 0 failed, 2 existing ignored**, across 129 result groups; process exit 0. The photographic gate ran with its explicit fixture environment. The new 22 tests include standalone FFI/null/cancel and compound-progress coverage. |

The final full-suite invocation was:

```sh
CARGO_TARGET_DIR=/private/tmp/rz-phase9-tests CARGO_PROFILE_RELEASE_LTO=false \
CARGO_BUILD_JOBS=4 RZ_RETOUCH_PHOTOGRAPHS=/private/tmp/rz-phase9c-photographs make test
```

This keeps release optimization while disabling cross-crate LTO only for test
binaries in a separate build directory; repository build settings are unchanged.
The production app, independent final-archive C comparisons and final UI/MCP
qualification use the normal LTO build. The full suite requires localhost access
for its existing agent/assistant mock servers. An earlier sandboxed run stopped
at that environmental permission refusal and is not counted as a passing gate.
Final receipts are `/tmp/rz-phase9d-rust-tests-isolated.log`,
`/tmp/rz-phase9d-build-verified.log`, `/tmp/rz-phase9d-lint-complete.log`,
`/tmp/rz9d-host-typecheck8.log`, `/tmp/rz9d-ui6.json`,
`/tmp/rz-phase9d-mcp-final.json` and `/tmp/rz-phase9d-native-mcp-final.json`.
The final core archive SHA-256 is
`0e523b28d7d2c7a3625025ca584dc1be903a07881842b753bab3aa480d486a58`.

### 9D-1 — retouch sampling

These are the historical 9D-1 measurements. The later 9D qualification above
supersedes its typed-source and geometry limitations. The 9A/9B/9C gates are unchanged: the full Rust
regression re-executes every earlier numerical, native, cache and refusal
contract on each run.

| Gate | Result |
| --- | --- |
| Full Rust regression | **1,268 passed, 0 failed, 2 existing ignored**, across 124 result groups — the 1,236 that existed before plus 32 new (18 sampling, 14 new-layer output), with no earlier test changed except the one refusal wording recorded below. |
| Formatting / all-target Clippy | Passed with `-D warnings`, all targets. |
| Authoritative Swift typecheck | Passed with 0 errors. |
| Release build | **BUILD SUCCEEDED**, 0 errors. |
| External MCP workflow | **7 groups passed** across both depths (6 at rgba8, 1 at rgba16), 13 refusals exercised, checkout inputs unchanged under `--require-current`, 4 fixture files byte-identical, all owned app PIDs exited. |
| Public MCP reference | Regenerated from an owned instance: **178 catalog tools — unchanged.** 9D-1 adds ARGUMENTS, not tools; a moved count would have meant something was added that should not have been. |

**The measurement the slice exists for.** P7-15's acceptance sentence is
"retouch under strong live adjustments without applying them twice", and the
probe makes it a number. On a photograph under a brightness adjustment layer,
cloning a blemish out with `sample: "all"` lands the patch at **207** where its
graded surroundings read **175** — the grade applied twice, a **31-code** error.
With `sample: "current_and_below"` the same stroke lands at **176**, within one
code of its surroundings.

The measurement is taken below clipping, and that is a correction rather than a
precaution: the FIRST run used a grade strong enough to saturate and reported
the doubled patch at 255, a floor that would have kept reading 255 however much
worse the double application got. The grade was reduced until the doubled value
was measurable, and the probe now asserts `double < 255` so the gate cannot
silently go back to reporting a floor.

**What each mode reads** is pinned against oracles that share no code with the
cut — the projection of a document with the layer above REMOVED, the layer's own
raw canvas image, and the document's own projection. `sample: "all"` is the
document's projection byte for byte and `sample: "current"` is
`rz_doc_layer_canvas_image` byte for byte, which is what makes the compatibility
claim below true rather than approximately true. The probe's fixture is built so
all three modes read three DIFFERENT pictures in the region the stroke samples
from. That too is a correction: the first fixture had the target opaque there,
where `current` and `current_and_below` legitimately coincide, and it passed
while reporting only TWO distinct results — a gate that would have kept passing
had the implementation conflated the two modes. The fixture now puts a hole in
the target over half the source region and a differently coloured layer beneath
it, and the probe asserts all three results differ.

**The cut is a contribution gate, not a visibility rewrite**, and that is the
one design decision worth recording. The obvious spelling of "composite the
stack without these entries" is to clone the layers and clear `visible`. It is
wrong: `doc_group::is_isolated` takes `has_visible_clipped_members`, so hiding a
clipped entry ABOVE the target flips the target's own enclosing group from
isolated to pass-through, and an adjustment confined to that group then reaches
the whole backdrop — moving pixels BELOW the target, which a sampling plate must
never do. That flip is deliberate, shipped behaviour for a hide, pinned by
`group_tests::clipping_over_a_pass_through_group_isolates_it`, whose last
assertion is exactly `hidden == open`. So `Cut` gates contribution at three
sites (`composite_level_into`'s two entry guards, `extent_of_level`, and
`resolved_member`, which resolves a cut member to an invisible placeholder kept
positionally) while `has_visible_clipped_members` and `is_isolated` keep reading
the document's own flags. `Cut::NONE` is every existing caller, so a projection
with no cut runs the statements it always ran.
`retouch_sampling::cutting_a_clipped_member_does_not_re_isolate_its_group` is
the test that separates the two, and it asserts both halves: the plate agrees
with the screen, and hiding the same entry really does move that pixel to the
155 a `visible`-rewriting implementation would have produced.

**New-layer output runs the shipped op, not a new one.** `heal_writeback::heal`
reads the destination in THREE roles — the `usable` gate, the contour term
`b = destination − source`, and the base of the final lerp — so a transparent
new layer refuses outright, and merely re-pointing the gate would leave the lerp
starting from zero and fade every repair below full strength toward BLACK.
Instead the existing entry point runs against a one-layer temporary document
made from the plate, at the caller's own strength, and the result is lifted onto
the new layer; nothing in `doc_heal`, `doc_inpaint`, `heal_writeback` or
`poisson` changes. The composite is then the in-place picture **byte for byte**
for a hard-edged footprint (Rust), and **within one code on 9 of 12,288 samples**
for a CoreGraphics-rasterized stroke (MCP) — that residue is double
quantization at the anti-aliased rim, which the byte-exact Rust case is what
pins it to. The deposit's alpha is the plate's own alpha, so a translucent area
stays translucent; where the picture is opaque, which is every photograph, the
composite is exact.

**Compatibility is per tool and per depth**, because the four tools' historic
defaults disagree: `clone_stamp`, `heal_stroke` and `patch_region` sampled the
composite and `spot_heal_stroke` and `content_aware_fill` sampled the layer. A
single design-wide default would have silently rewritten every recorded call, so
each tool keeps its own and the probe asserts all three. `sample_all_layers`
keeps its name, units and meaning as a legacy alias; co-presence with `sample`
is ACCEPTED when the two agree and refused only when they CONTRADICT — which is
not leniency but a requirement, since the app's own recorder writes both
spellings so that a step recorded now still replays on a build that predates
`sample`. `current_and_below` has no boolean spelling, so a result reports the
boolean only where it can tell the truth rather than reporting a lie.

**Defects this slice found and fixed.**

- **A clone stroke that deposited nothing minted a blank layer.** `painting_layer`
  ends in `with_layer_pixels`, which accepts replacing a buffer with an equal
  one, so an empty overlay onto a fresh transparent layer produced an undo step,
  an entry to notice and delete, and no change. Found by writing
  `a_gesture_that_deposits_nothing_creates_no_layer`; fixed by testing the
  overlay's emptiness before any layer exists.
- **`content_aware_fill` validated document state before its arguments.** An
  unsupported `output` was masked by "there is no selection to fill", sending
  the caller to make a selection, call again, and only then meet the real
  refusal. Found by the probe; the argument check moved ahead of the state
  check.
- **`RetainedImagePolicy.pixelTools` named tools that do not exist.** It listed
  `"spot_heal"` and `"patch"` where the dispatch keys are `spot_heal_stroke` and
  `patch_region`, and omitted `dodge_burn` entirely. `spot_heal_stroke` was the
  live hole: it commits through `DocumentJobs` rather than `performPixelEdit`,
  whose own retained-source check covers the synchronous paths, so an agent
  could spot-heal a layer with a retained master and silently detach it.
- **`annotatePrecisionCatalog` published arguments the byte handlers ignored.**
  `source_layer` and `sample_all_layers` were injected into `clone_stamp` and
  `heal_stroke`, and the byte path read neither — an 8-bit caller passing
  `sample_all_layers: false` to `clone_stamp` got composite sampling with no
  refusal and no note. Now the byte path honours the argument.
- **The typed gate read one spelling of two.** `DocumentPrecision.refusal`
  checked `sample_all_layers` only; `sample: "all"` would have slipped past it.
  Both are checked now, and the old rule that demanded an explicit
  `sample_all_layers: false` — an argument whose only legal value was its own
  default, which the recorder then had to inject in a second place where the two
  writers could disagree — is replaced by resolving an absent argument to the
  one mode typed retouch has.

**Deliberate changes to existing behaviour**, each recorded rather than
absorbed:

- **The sampled plate is tagged with the destination's space, not the pixels'
  own tag.** The two differ only for a LUT/no-B2A profile, where
  `ColorProfile.drawingSpace` answers sRGB while the pixels wear the document's
  tag — and there CoreGraphics silently converted the sampled texture on its way
  into the overlay, against this app's own stated rule that a sampled colour
  converts nowhere. On such a document a clone's pixels change, from converted
  to unconverted. The Patch tool's drag still tags for the SCREEN, because that
  buffer is drawn into the view rather than into an overlay.
- **One refusal sentence moved.** Content-Aware Fill on a layer with nothing
  solid to sample said "turn on Sample All Layers", a control that no longer
  exists; it now names Current and Below and All Layers, and
  `content_aware_tests` moved with it.

**The display-preview clause, stated precisely.** No display image reaches
committed pixels: every byte-depth gesture samples a core-built plate, and the
typed path commits through `PrecisionPixelEdit` against a captured raster leaf.
The claim is NOT that no display image is latched anywhere — a 16-bit drag still
latches `canvas.image` for its ghost, because `onStrokeBegin` routes a precision
document to `beginPrecisionStroke` before reaching the plate latch. Those pixels
are never read back. Recorded with the two open review findings above rather
than rounded up to the stronger sentence.

**Not started, and not half-started.** Fractional-offset sampling, source
overlays, source transforms and destination expansion (P7-16) and the typed
spot-heal and red-eye migration (P7-17) are untouched. The typed path keeps its
single-raster-leaf rule and refuses every other mode by name.

### 9C — noise cleanup

These are the historical 9C qualification measurements. The later 9D entry
above supersedes its retouch boundaries; the whole Phase 9 remains open. The 9A and 9B gates are unchanged except where the shared
suites cover them: the full Rust regression re-executes every earlier numerical,
native, cache and refusal contract on each run, and the filter-stack MCP probe
now qualifies 9B and 9C together rather than forking a second 500-line copy of
itself.

| Gate | Result |
| --- | --- |
| Full Rust regression | **1,236 passed, 0 failed, 2 existing ignored**, across 122 result groups. 45 new tests — 12 rank-numeric, 11 denoise-numeric, 14 neighborhood/native/cache/refusal, 8 destructive-twin — less the one in-`src` unit test removed below. |
| Formatting / all-target Clippy | Passed with `-D warnings`, all targets. |
| Authoritative Swift typecheck | Passed with 0 errors. |
| Release build | **BUILD SUCCEEDED**, 0 errors, first attempt. |
| External MCP workflow | **56 groups passed** across both depths (30 at rgba8, 26 at rgba16), 72 refusals exercised, 729 checkout inputs unchanged under `--require-current`, 5 fixture files byte-identical, all owned app PIDs exited. |
| Photographic quality gate | **Three real photographs, every frozen gate passed**, plus the RAW-independence measurement. Details below. |
| Public MCP reference | Regenerated from an owned instance: **178 catalog tools — unchanged.** 9C adds OPERATORS, not tools; a moved count would have meant something was added that should not have been. `apply_filter` now lists `reduce_noise` and `dust_and_scratches`. |
| Phase 9A MCP regression | **Could not be re-run on this machine.** Recorded below rather than claimed. |

**The photographic gate**, which is the clause of the 9C plan sentence no
synthetic plate can discharge. The corpus is three real photographs: one
committed in this repository (`samples/portrait-exif6.jpg`, so an offline run is
still a real measurement) and two checksum-pinned images already used by the
Phase 6 quality fixtures, downloaded into scratch and never committed. The
corruption — correlated luminance grain at 6/255, wider chroma grain at 12/255,
impulse dust on 0.4% of pixels, seed `0x9E3779B97F4A7C15` — and every threshold
were frozen before any output was observed, and the probe carries its own
Gaussian and its own generator so it never measures a filter against itself.

Every comparison is made at a MATCHED operating point, because texture retention
is otherwise gameable by simply filtering less: a plain `gaussian_blur` is
bisected until it removes the same noise, and the whole bisection trace is
recorded so the match is auditable rather than asserted.

| Measurement | portrait | fur | astronaut |
| --- | --- | --- | --- |
| Texture retained, `reduce_noise` vs a Gaussian at the same residual | **0.950** vs 0.647 (σ 0.54) | **0.989** vs 0.568 (σ 0.61) | **0.991** vs 0.423 (σ 0.75) |
| Halo introduced at an edge, on the UNCORRUPTED photograph | 0.79 codes, 0 reversals | 0.63 codes, 0 reversals | 0.36 codes, 0 reversals |
| Denoise-then-sharpen vs sharpening alone: residual | 13.7 vs 28.1 | 11.6 vs 29.6 | 4.9 vs 14.6 |
| …and halo | 11.7 vs 16.2 | 81.1 vs 87.1 | 70.3 vs 70.7 |
| Dust removed, of the pixels an impulse actually struck | **100%** | **100%** | **97.8%** |
| Texture retained doing it, vs a Gaussian at the same dust removal | **0.472** vs 0.016 | **0.575** vs 0.010 | **0.636** vs 0.014 |
| …and whether a blur could reach that removal at all | matched | **no blur in the bracket reaches it** | **no blur in the bracket reaches it** |
| Rank artefact: samples outside their own window | **0** | **0** | **0** |
| Hue rotation at that artefact, radius 1 / radius 4 (recorded, not gated) | 29.7° / 41.5° | 31.0° / 157.4° | 122.6° / 178.7° |
| Re-edit A→B→A eight times across save and reopen | bit-identical | bit-identical | bit-identical |

That halo row is the one a guided filter could genuinely have failed: a local
linear model is not confined between its source and its low-pass and can halo at
an epsilon mismatch. Measured on the clean photograph, where a halo is the only
thing left to measure, it introduces under one code and reverses no gradient.

**"RAW noise controls alone do not close this workflow"** is measured, not
asserted, in four parts. On an ordinary photographic import the RAW tools refuse
by name — there is no recipe — while `dust_and_scratches` still removes every
one of 635 struck pixels and `reduce_noise` still changes the picture. A masked
cleanup leaves every pixel outside its coverage EXACTLY unchanged, which the
recipe cannot express at all: RAW noise reduction has no coverage, no ordering
and no per-entry blend mode. On a real RAW file whose decoder does expose
`luminanceNoise` and `colorNoise`, the two mechanisms are distinct and compose
(recipe-only, live-only and both give three different renderings), removing the
live stack returns the recipe-only rendering EXACTLY, and editing the recipe
leaves the filter descriptor byte-identical. And the dust gate above shows what
a smoothing control — which is what RAW noise reduction is — costs: a Gaussian
matched to the same dust removal destroys 98–99% of the texture the rank filter
keeps.

**Five defects in this probe were found by running it or by the review below,
and are recorded rather than quietly fixed**, because each one made the gate claim something it had not
measured — the same failure the 9B review found in the oracle's detectability
claim. (1) Halo and the rank hue artefact were measured on the CORRUPTED plate,
where both are dominated by whatever noise the filter left behind; they are now
measured on the clean photograph, where the operator's own edge behaviour is the
only thing present. (2) Dust removal counted every pixel differing from the
reference, so the median's ordinary effect on clean content was counted as
unremoved dust and the first run reported dust cleanup making the image WORSE;
it now counts only the pixels an impulse actually struck, which the corruption
records. (3) "Residual noise" was the raw RMS difference from the reference in
the quietest regions, which also carries the filter's low-frequency bias on
whatever gradient those regions contain — on one photograph that put a floor
under the number no admissible strength could cross, and would have been read as
the operator failing to denoise; it is now the RMS of the HIGH-FREQUENCY part of
that difference. (4) Flat regions were selected by an absolute variance
threshold, a synthetic-plate idea that no real photograph cleared: two of three
photographs yielded no region at all and were skipped. Regions are now the
quietest rects with their measured variance recorded. (5) The dust gate recorded
its Gaussian match without requiring it, so the one comparison the module exists
to make could silently be made against an unmatched blur; it is now gated, with
an explicit second branch for the case that actually occurs on two of the three
photographs — that no blur in the bracket reaches the rank filter's dust removal
at all.

Two frozen thresholds were restated, with both the original and the measured
value on the record. G4's halo margin was an absolute 0.5 code; the quantity it
bounds turned out to be 70–87 codes on a real photograph under a 1.5× unsharp
mask, so an absolute half-code margin was a scale error rather than a gate, and
the halo half of G4 moved to the clean photograph with the noise half staying on
the corrupted one. G6 gated the hue rotation at 8°, a number nothing justified;
the operator's real bound is that no output sample leaves its own disc's
per-channel box — already pinned EXACTLY in `live_filter_rank_numeric.rs` — so
the photographic gate now asserts that invariant on real photographs and RECORDS
the rotation at two radii instead, which is what the artefact's growth with
radius actually needs.

**The Phase 9A MCP regression could not be re-run, and this is not a 9C
regression.** Its probe opens a real 5 MB camera NEF after about forty documents
in one app, and on this machine that call now hangs with the main thread waiting
on a worker that is inside Core Image's Metal RAW render — no Rasterize frame
below `RetainedRawDecoder.develop`. A 300-second timeout does not help, an idle
machine does not help, and a FRESH app decodes the same NEF in 0.6 seconds. The
committed Phase 9B build (`33855da`) was rebuilt in a clean worktree and fails
identically, at the same probe line, on the same call. The 9A contracts remain
covered by the full Rust regression, which re-executes them on every run, and by
the filter-stack probe's `phase_9a_compatibility` scenario, which passes. The
9A probe's HOST stage is separately stale for a reason that predates 9C: it
still names `LiveGaussianFilterSheetController` and `showLiveGaussianBlur`, which
Phase 9B renamed. Repairing that probe against the 9B sheet is real work and
belongs with whoever renamed them; making it merely compile would have produced
a probe that fails for a different reason.

One residual is recorded and deliberately not closed. A 9C-authored preset in
the shared default filter-preset directory makes an older build's ENTIRE preset
library unreadable — list, apply, save and delete — because the library is
whole-library strict and `readEntries` throws on the first unrecognized operator
with no skip-and-continue. No version gate helps, since the preset schema is
unchanged and correctly so; the fix is a change to that library's strictness
contract, which is not 9C's to make.

## Corrections the 9D-1 review found

Unlike 9B's and 9C's, this review ran BEFORE the implementation, against a
written design: seven reading dimensions — composite semantics, the
double-application acceptance test, the Poisson write-back, cost and bounds,
the typed path, compatibility and replay, UI and gesture — with every serious
finding then put to an independent verifier whose default was REFUTED. It
reviewed 75 candidates, verified the 57 non-minor ones individually, confirmed
27 and refuted 5 outright. The ten confirmed blockers are the reason the design
that shipped is not the design that was written; each is named because the
reasoning generalizes.

- **A sampling cut must not be spelled `visible`.** The first design cleared
  `visible` on the entries a mode excludes, reasoning that the compositor
  re-derives clip runs positionally so nothing would move. It proves the wrong
  thing: it proves the plate equals *the document with those layers hidden*, and
  in this compositor hiding a layer ABOVE the target can change what is
  UNDERNEATH it. `is_isolated` takes `has_visible_clipped_members`, so the cut
  would flip the target's own enclosing group from isolated to pass-through and
  an adjustment confined to that group would reach the whole backdrop. That flip
  is deliberate, shipped behaviour for a hide —
  `group_tests::clipping_over_a_pass_through_group_isolates_it` ends on exactly
  `hidden == open` — and the wrong answer for a plate, whose job is to be the
  picture the user is looking at minus some entries' CONTRIBUTION. The verifier
  also corrected the fix: the cut has to reach three sites, not the two
  proposed, because `resolved_member` feeds a `visible` filter of its own.
- **The membrane write-back reads the destination in three roles, not one.**
  The design proposed giving it a distinct reference plane. `heal_writeback`
  uses the destination as the `usable` gate, as the contour term
  `b = destination − source`, AND as the base of the final lerp
  `out = old + weight·(healed − old)`. Re-pointing the first two leaves the
  third reading a transparent layer, so `old = 0` and every strength below 1
  fades the repair toward BLACK — correct-looking at 100 %, which is exactly how
  it would have shipped. It would also have broken three written contracts
  (`heal_writeback`, `precision_heal` and `doc_heal` each promise alpha never
  moves) in a function the byte and typed paths share. The shipped design edits
  none of it: the existing op runs against a one-layer temporary document made
  from the plate.
- **Strength belongs in the solve, not in the deposit's alpha.** The design said
  the new layer's pixels carry "coverage as alpha". `strength` lives ONLY in
  `heal_writeback`'s write weight, so depositing at coverage would have dropped
  the tool's only strength control entirely — a 50 % Healing Brush depositing at
  full strength — and, because the covered set spans alpha 128–255 while the
  write plane is a flat 255, would have faded the repair across the rim band the
  hard cut exists to keep at full strength. On a 40 px soft dab that band is a
  visible ~5 px ring, not the one-pixel edge the finding first assumed.
- **A deposit is seamless only if the plate IS the backdrop beneath it.** The
  join is invisible because the solve returns the destination exactly on the
  contour; deposited into a separate layer at full coverage, that only holds
  where the layer's actual backdrop equals the reference. The design offered six
  sample×output combinations and guaranteed it in none. Hence the refusal that
  now names the enclosing group, and the rule that a new-layer repair always
  takes the current-and-below plate as its reference whatever the gesture
  samples.
- **Two blockers about a second implementation and an unbounded one.** The design
  proposed a new full-frame plate builder; the core already had a tiled,
  cancellable, scratch-reserving one for this exact job, and `core/CLAUDE.md`
  says one implementation per algorithm. The verifier corrected the finding's own
  arithmetic — the saving is a 384 MB avoidable transient and ~5× peak, not the
  "400×" claimed — and caught that the `pointwise` predicate has to run over the
  CUT stack or `ignore_adjustments` is forced full-frame by the very adjustment
  it is cutting.
- **Four blockers in the compatibility rules, which were mutually
  unsatisfiable.** Refusing `sample` and `sample_all_layers` together would have
  made the app's own newly recorded 16-bit steps unreplayable, because
  `EditorViewController+PrecisionPaint` injects the boolean after the factory
  has run; the typed handler simultaneously REQUIRED that boolean, so no
  argument set worked at all; and `sample` had no stated default, while no single
  default preserves today's pixels because the four tools' legacy defaults
  disagree. The shipped rules — a per-tool default, an alias refused only on
  contradiction, and an absent argument resolving to the one typed mode — are all
  three of those corrections.

Five findings were refuted, among them a claim that a new-layer heal cannot
reproduce the in-place result wherever the destination is not fully opaque
(true of the composite's alpha, not of its colour, and the deposit carries the
plate's own alpha for exactly that reason) and a claim that in-place retouch
silently destroys a layer's live filter stack (the app refuses retouch on a
retained-source layer before the core sees it).

**Two confirmed findings are NOT closed by this slice, and are recorded rather
than absorbed.**

- **The plate has no host-side admission.** `rz_doc_retouch_source` takes an
  `RzJobControl` and reserves against it, but no caller passes one, so the
  reservation is `JobContext::NONE` and a no-op. A Clone Stamp mouse-down used to
  cost zero pixels — the snapshot was a zero-copy provider over an image the
  canvas already held — and now costs one full canvas composite, outside the
  app's admission system. The MCP heal and patch paths already paid that cost
  before this slice (`document.projection ?? doc.flattened()`), so nothing there
  regressed; the interactive Clone Stamp is the new cost. The review's own
  better answer is recorded with it: for `sample: "all"` on a byte document the
  authoritative projection Swift is already holding for that revision IS the
  plate, so that mode could be free rather than merely admitted.
- **A 16-bit drag still latches the display image for its ghost.** `onStrokeBegin`
  routes a precision document to `beginPrecisionStroke` before reaching the plate
  latch, so `strokeSourceImage` answers nil there and the canvas falls back to
  `canvas.image`. Those pixels reach no commit — `emitStrokeUpdate` returns early
  under `precisionStrokePreview` and the ghost floods the overlay's alpha, while
  the commit goes through the typed core op — so the phase's rule holds for every
  pixel that lands in a document. But the latch happens, and the precise claim is
  "no display image reaches committed pixels", not "no display image is latched
  anywhere". Both belong to the typed retouch work in P7-16/P7-17.

## Corrections the 9C review found

An adversarial review of the slice — six reading dimensions, every finding put
to three independent refuters and kept only if a majority failed to refute it —
reviewed fourteen candidate defects and confirmed five. They are fixed here, and
each is named because the reasoning generalizes.

- **A guided filter divides 0/0 when the strength underflows.** `epsilon` is
  `luminance * luminance`, and `luminance` is admitted anywhere in [0, 0.25] by
  the core, the destructive twin and the MCP schema alike. Any value below about
  1.5e-162 squares to EXACTLY zero while `halves()` still reports the luminance
  half active, and on a window with no variance — guaranteed on an opaque black
  region, common wherever `S2/S0 − m·m` cancels — the slope became 0/0. One NaN
  then poisons the running box accumulator for the rest of its row and every row
  below it, so an in-range, documented parameter made the document
  UNRENDERABLE rather than merely unfiltered. Zero variance now takes the mean
  alone, in the product and in the oracle's own re-derivation of the same law.
- **A forecast that says "independent of radius" has to be.** The per-pixel cost
  genuinely is, but each box pass re-seeds its accumulators once per row and once
  per column with `2r+1` reads, and on a plane narrower than the window that term
  dominates — at the image ceiling in a tall thin strip it was about four times
  the charge. `plan_bounded` now charges it, and the module doc says PER-PIXEL.
- **A clamp test that never reached the clamp.** The chroma slope test asserted
  its own precondition on the RAW ratio `cov/var`, about 6 on that fixture, while
  the product clamps the REGULARIZED `cov/(var + eps)`, which never reached 2.1
  there — so `CHROMA_SLOPE_LIMIT` could have been raised to a thousand and the
  test would still have passed, the oracle agreeing because its clamp was equally
  inactive. The test now computes the gated quantity itself by direct summation
  and asserts it exceeds the limit before comparing.
- **A structural gate that could not detect the thing it guarded.** Above the
  exact-oracle band the window geometry was pinned only by a constant plane —
  and every window, right or wrong, returns the same constant. A linear ramp does
  not: a box average returns it exactly at an interior pixel only if the window
  is symmetric, so an off-by-one shifts the mean by half the slope. The new ramp
  gate runs at radii the direct oracle cannot reach, over a plate wider than
  twice the window, with the strength chosen so the local model is the window
  mean and therefore maximally sensitive. Writing it also found two defects in
  ITSELF before it found none in the product: a margin of one window where the
  coefficient averaging reaches two, and a fixture whose `as u16` wrapped.
- **A matched comparison that was not required to match.** The photographic
  texture gate exists because the comparison is meaningless unless the Gaussian
  removes the same amount; the denoise gate enforced that and the dust gate did
  not, while recording a match error of 17 and 92 impulses on two photographs.
  It now passes only if the blur matched the rank filter's dust removal — or if
  NO blur in the bracket can reach it, which is itself the result and is recorded
  as such. Two of three photographs take that second branch.

Two more were confirmed real but minor and are closed with them: the destructive
Reduce Noise refused an out-of-range `luminance` by the name "strength", which no
caller has; and the live membership gate asserted a min/max RANGE, which an
averaging implementation would also satisfy — it now asserts true membership,
the tolerance-free structural check that gate exists to be.

Nine further candidates were refuted, each by a majority of independent
refuters — among them a claimed unbounded allocation in `disc_offsets`, a
claimed under-charge in the denoise work forecast, and a claimed silent no-op
from the two new Filters menu items in 16-bit mode, which is the established
behaviour of every neighborhood filter here rather than anything 9C introduced.

### 9B — the ordered stack

All required 9B gates pass on the source below. The whole Phase 9 remains open;
9C has not started. The 9A gates in the next section are unchanged and were not
re-run except where the shared suite covers them: the numerical, native, cache
and refusal contracts of the single Gaussian are re-executed by the full Rust
regression on every run, and the 9A MCP suite's catalog and canonical-Actions
assertions were updated for the wider tool set and the new recorded step.

| Gate | Result |
| --- | --- |
| Full Rust regression | **1,192 passed, 0 failed, 2 existing ignored**, across 118 result groups. Includes 26 new tests: 13 stack-numeric, 8 stack-native/cache/refusal, 5 destructive-twin. |
| Formatting / all-target Clippy | Passed with `-D warnings`, all targets. |
| Authoritative Swift typecheck | Passed with 0 errors. |
| Release build | **BUILD SUCCEEDED**, 0 errors. First attempt failed on a SILGen-only diagnostic (`closure captures 'layer' before it is declared`) that `make typecheck` cannot see — see the note below. |
| External MCP workflow | **34 groups passed** across both depths (18 at rgba8, 16 at rgba16), 46 refusals exercised, 722 checkout inputs unchanged under `--require-current`, 5 fixture files byte-identical, all 6 owned app PIDs exited. Receipt: `/tmp/rz-phase9b-mcp.json`. |
| Public MCP reference | Regenerated from an owned instance: **178 catalog tools** (171 + the 7 new live-filter/preset tools), `apply_filter` now lists `unsharp_mask` and `high_pass`. |
| Phase 9A MCP regression | **30 groups passed** against the 9B build, no checkout drift, all owned apps exited. Receipt: `/tmp/rz-phase9a-mcp-9b.json`. |

Three 9A probe assertions were updated, each only after dumping the actual
saved record to confirm the difference was the intended one rather than a
regression. The persisted descriptor is now `schema_version` 2 with an explicit
`opacity` and `blend_mode`, while the native document version stays **23** —
the widening spent reserved room and changed no framing. A stack created
through the 9A-shaped `set_live_filter` entry point now takes the uniform
`<op>-<n>` identity the three operators share, so a fresh single Gaussian is
`gaussian_blur-1` rather than `gaussian`; descriptors SAVED by 9A keep their own
ids and schema, which is what keeps their coverage aligned. And
`get_live_filter` reports `masks` — a list of `{filter_index, mask}` — instead
of a single `mask`, because a stack carries one plane per entry.

`make typecheck` reported 0 errors while the Release build failed: `swiftc
-typecheck` stops before SILGen, so SILGen's own diagnostics never run, and
adding `-wmo` does not change that. A clean typecheck does not prove the app
compiles — only `make app` does. Both CLAUDE.md files now say so.

Two defects in the 9B probe itself were found while running it, both of which
made it claim coverage it did not have — the same failure the review found in
the oracle's detectability claim, so they are recorded rather than quietly
fixed. `fallback()` returns `png_samples`' DICT, and the blend/opacity
distinctness check did `tuple(...)` on it, comparing dict key names, which are
identical for every render; it now compares width/height/depth plus the sample
sequence. And `import fixtures` bound `precision-probe/fixtures.py` (32x12)
instead of this probe's own (40x28) — five probe directories define that module
name — so every recorded geometry guard refused; it is now loaded by explicit
path like the probe's other helpers.

### 9A — one retained Gaussian

All required 9A gates pass on the final source and artifacts below. Pixel kernels
and native framing were frozen before the last two qualified Swift integrity
corrections. Phase 9B kept every one of these bounds and added its own.

| Gate | Result |
| --- | --- |
| Full Rust regression | **1,166 passed, 0 failed, 2 existing ignored**, across 115 result groups. Includes 19 new tests: 7 numeric, 11 native/cache/refusal, 1 work-plan unit. |
| Formatting / all-target Clippy | Passed; 342 core/build inputs and the archive unchanged. |
| C ABI | All 22 new exports exercised, with null/error/cancel/no-op, native 23/24, source/profile/mask and cache checks. Direct Gaussian maximum error 0 stored codes under its fixed 1-code bound. |
| Authoritative Swift typecheck / Release | Fourth checks passed; 360 typecheck and 596 build input file sets/hashes unchanged. |
| Actual-source host | **115 groups passed**, 732 used inputs and 367 compiled inputs unchanged; owned PID 4371 exited. |
| Independent host numeric checks | 48 rows, 239,616 components; maximum error 1 stored code within the fixed per-depth 1-byte / 3-, 4- and 6-u16 bounds. Six analytical impulse cases exact. |
| Source/master preservation | **49,264,664 exact components**, plus exact native source ICC, mask payloads and bypass samples. Includes genuine 6 MP RAW/local and host movie sources. |
| Actual UI / navigation | 16 modal UI groups plus two genuine-camera navigation groups; actual 2× screen verified with available scales [2, 1]. Full preview, controls and decisions are visible in owned captures. |
| Fresh-process close | Runs before any live-filter work entrypoint, creates/saves/reopens using native wrappers, warms cache through the existing retained transform, then closes while holding native owners. Both depths change cache retention from 38,103 bytes to 0, advance generation and preserve pixels. |
| Resource/cancellation/refusal | Native work was observed before active cancellation or stale-owner changes. Final active cancellation took 0.358709 s for byte and 0.002297 s for typed, within the fixed 5 s gate. Scratch returns to zero and late cache storage remains empty. Actual UI queued cancellation/close are separate lifecycle tests. |
| RAW stage forecast | A valid independent 1×24,000 native fixture with feather 256 has 444,838,132 bytes of native scratch, exceeding the old 35,090,432-byte camera peak. The new forecast is 478,392,564 bytes, including the 32 MiB RAW envelope. No giant pixel render was performed. An unknown filter refuses before worker entry. |
| Final external MCP | **30 groups passed**, with 97,214,464 exact source components, 171 catalog tools, 733 current inputs/file sets unchanged and all six owned app PIDs exited. Covers native/Actions/snapshot/crash recovery, source deletion, RAW/local/frame coexistence, strict refusal and full-versus-compact unknown-state inspection. |

Final workflow receipts are `/tmp/rz-phase9a-host-final3.json`, its immutable
scratch/source snapshot and `/tmp/rz-phase9a-mcp-final2.json`. The independent root
audit `/tmp/rz-phase9a-final-audit.json` verifies every final source/artifact set,
29 fixture files and all seven owned process exits. Core, C and build receipts are
`/tmp/rz-phase9a-core-summary.json`, `/tmp/rz-phase9a-c-abi.json`,
`/tmp/rz-phase9a-typecheck-fourth.json` and `/tmp/rz-phase9a-app-fourth.json`.
The public MCP reference is generated from the complete 171-tool captured catalog;
`/tmp/rz-phase9a-public-reference.json` records exact regeneration.

| Qualified artifact | SHA-256 |
| --- | --- |
| Fourth Release executable | `382fa0ace8b596ae40f5527d149f59b771ca795b685da6e294fd037cb35bfd06` |
| Target15 core archive | `82987984d1eb794453db74f1cbe617d8944379604580c4bdfdce4153cc148f96` |
| Final host executable | `49422f7a9fb0a1ac93bc026a1eda84d6d2b47518dc89804706d4e9cd688f8682` |

The input manifests include source/header/build configuration and independent
fixtures/oracles. Test apps use dummy credentials, private Actions/recovery/RAW
preset stores and non-default ports. Only owned processes and windows are operated
on; host PID cleanup is recorded separately from external app PIDs.

## Historical integration evidence

The first typecheck and preliminary C gate passed, and independent postprocessing
of the second host run passed 48 numeric rows/239,616 components, six analytic
impulses and 25,200,664 exact original-master components. The full host remained
failed: screenshot review found the shared card's 420-pixel default crushed the
preview and made native pan miss it. The product now explicitly requests a
1,010-pixel card, and the probe checks usable width before gestures. Two resource
witnesses were corrected to observe native progress independently of UI timing.
The initial numbers remain historical and do not replace final measurements.

The fourth host then passed 110 groups, including the corrected full-width UI,
and the planner witness brought a later snapshot to 111. The standalone native
fixture writer initially embedded ICC inside a pixels-only PNG; that fixture
correctly refused and was corrected without changing the product or bounds.

A final inspection review found that unknown opaque filter JSON could retain a
large parsed graph across compact stack rows. Unknown rows now retain presence,
editability, mask count and `descriptor_omitted: true`; known Gaussian scalar
descriptors remain visible. Dedicated `get_live_filter` keeps full opaque metadata
under an owned 48 MiB parse/output lease through serialization. Denied-read
presence checks increased the host suite to 113 groups.

Finally, cache clearing moved into `ImageDocument.close`, covering native reopen
and ordinary retained transforms even before the live-filter UI or tools are used.
The lazy feature-only observer was removed; the new first-in-process checks bring
the final suite to 115 groups. Prior passing host/MCP reports are preserved as
`/tmp/rz-phase9a-host-before-opaque-fix.json` and
`/tmp/rz-phase9a-mcp-before-opaque-fix.json`. No numerical threshold was relaxed.


## 9F — selected tone and scoped analysis

The [9F status](phase-9f-selected-tone-status.md#qualification) records native
selected writeback for every exposed destructive tone/spatial operator, fine
Levels/Threshold/Curves entry, independent whole-target/weighted-selection Auto
analysis, and named composite/raw-layer/backdrop readouts. Exact endpoint counts
and code/coverage sums inspect the full scope independently of display-bin stride.
Legacy byte outputs and Action parameter meanings remain intact. Native scalar
editing and higher-precision document selection coverage are qualified separately in 9G.

Independent Rust, actual UI, external MCP/Actions/Undo, native/snapshot/recovery
and original-byte-baseline checks verify P7-06/P7-07. Full Rust passes 1,321 tests;
actual UI passes 12 groups in both depths. The native format and 180-tool count
remain unchanged. The linked status records final app/source hashes, exact
scope/alpha policies, resource admission and qualification boundaries.


## 9G — native selections, masks and planes

The [9G status](phase-9g-selections-masks-status.md#qualification) records the
complete P7-08–P7-11 qualification: scalar brush/fill/filter/Auto/morphology and
RGBA-plane writeback, raw/effective display and exact scalar histogram counts,
Apply Image/Calculations/luminosity channels, typed source selections and native
selection/Quick Mask history. The additive u16 ABI preserves legacy byte
semantics; editing-state v3 stores exact native coverage/view state and native
v25 stores channel display metadata and auxiliary provenance.

All 1,354 Rust tests pass, with two existing ignored tests; Clippy, authoritative
Swift typecheck and normal-LTO app build pass. Independent MCP covers 31 native
filters, 18 scalar Auto cases and complete Actions/Undo/persistence; actual UI
checks controls, previews, gestures, thumbnails and captured scalar histograms.
Eight auxiliary EXIF orientations for two channels preserve exact native
samples through native/snapshot/crash recovery. Legacy v1/v2 archives and
original byte scalar outputs remain exact; malformed packages refuse. The
linked status contains the final app/input hashes, ULP-only normalized display
transport allowance and explicit source-precision/decoder boundaries. There are
181 MCP tools. Phase 9H is qualified below; P7-12/9I remains open.


## 9H — native paint and fill

The [9H status](phase-9h-paint-fill-status.md#bounds-and-qualification) verifies
P7-13/P7-14: native pressure size/opacity and actual airbrush deposits, versioned
canonical Actions, all 27 paint modes, mode-free erasure, color-match bucket
fill, linear/radial gradients, six Dodge/Burn modes and precise raw-storage
expansion. Stroke and selection gates stay independent through native writeback;
Quick Mask retains canonical dabs and captures state before its first worker.

The full Rust regression passed 1,369 tests; the final isolated bucket-seed
correction passes its complete 16-case affected FFI suite. Required Clippy,
current-source typecheck and the normal-LTO app pass. Final actual-source UI
covers nine groups, independent pressure/coverage references, real Timer and
Cancel controls, stale capture refusal, exact Actions/Undo and native history.
The external MCP probe adds six scalar targets, P3 authored-color counterparts,
all paint blends and native/snapshot/crash recovery; 60 immutable pre-9H byte
and typed cases remain exact. The linked status records final source/app/archive
hashes, receipts and the explicit 50,000-dab/work limits. The public catalog
remains 181 tools. Eighteen carryovers are verified and 18 remain open;
P7-12/9I and the complete Phase 9 gate remain open.


## 9I — cutout research and internal native assembly; gate open

The [9I status](phase-9i-cutout-status.md) preserves all original references,
thresholds and legacy results, and records five candidates declared before
scoring. Automatic prior values are soft evidence; only independently supplied
user labels are hard constraints. Neither reference alpha nor reference
foreground enters candidate inference. Each candidate retains all 24 analytic,
24 crop and 42 full-image cases, including refusals.

| Candidate | Alpha passes: analytic / crop / full | Combined passes: analytic / crop / full | Refusals: analytic / crop / full |
| --- | --- | --- | --- |
| Global soft prior v1 | 13 / 0 / 0 | 1 / 0 / 0 | 0 / 24 / 42 |
| Tiled soft prior v2 | 12 / 12 / 20 | 1 / 0 / 0 | 0 / 0 / 0 |
| Source-protected tiled v3 | 12 / 12 / 20 | 1 / 3 / 4 | 0 / 0 / 0 |
| Source-protected stable prior v4 | 12 / 12 / 18 | 0 / 3 / 4 | 0 / 0 / 0 |
| Joint Bayesian alpha/color v5 | 12 / 4 / 3 | 2 / 0 / 0 | 6 / 0 / 24 |

V4 retains every supplied prior exactly when no explicit user label overrides it;
its unchanged alpha and failed foreground/recomposition metrics isolate the
remaining evidence problem. The documented legacy reference-alpha color rows
use the original unprotected foreground estimator, not v3/v4's protected path.
The bottle reference remains silhouette truth, with no physical photographic
transmission truth or independent background capture available.

The [joint Bayesian v5 experiment](phase-9i-bayesian-v5-status.md) improves
analytic color separation but damages opaque photographic interiors. Its
boundary-derived single-Gaussian color populations are unsuitable for estimating
all interior pixels; full portraits also exhaust the fixed spatial-search cap.
Every refusal remains a failure. Independent replay preserves all 120 guided,
sampled and original closed-form case records exactly apart from timing.

The additive internal native assembly shares exact RGBA16 foreground/source
alpha and copies independent u16 refinement coverage into an editable mask.
It follows the existing top-root insertion/original-root hiding rules and
central Undo publication. Native coverage owners retain their registered lease;
capture uses the qualified 9G document/target/selection guards without producing
a byte source. Tiny positive alpha/matte products remain represented separately.
Native/snapshot/recovery functionality is distinct from estimator qualification;
the detailed status separates isolated receipts from integrated verification.

The [execution continuation](phase-9i-execution-status.md) adds claimed single-use
parent controls, owner-close forwarding, controlled byte record/string copies,
chunked shared byte projection and native render/preview cancellation. Context
metadata parsing is admitted before execution; individual JSON parses and
CoreGraphics calls retain documented cancellation boundaries. No format,
Actions schema or public tool is added.

Remaining work includes defensible missing-detail evidence, source-only
foreground reconstruction that passes the original recomposition and opaque
protection gates, photographic transmission evidence, and qualification of the
typed estimator, before/after and reusable-mask workflow. No advanced controls,
MCP or Actions path is enabled. **9I/P7-12 and Phase 9 remain open; eighteen
carryovers are verified and 18 remain open.**
