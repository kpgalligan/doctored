# Phase 9 — editable photographic filters and retouching (ADR)

- **Status:** Accepted — open items remain (9A–9H closed within scope; 9I / P7-12 gate half open)
- **Phase / dates:** 9A–9D recorded here, 9E–9I summarized with their own ADRs; 2026-09-11 → 2026-09-12; issue records added through October 2026
- **Originals:** [archive/phase-9-live-filters-status.md](../archive/phase-9-live-filters-status.md) — the phase's full history, to 2026-09-26, with review findings; [archive/2026-10/phase-9-live-filters-status.md](../archive/2026-10/phase-9-live-filters-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `c725d88` (9A), `33855da` (9B), `decada4` (9C), `01a5e51` (9D-1), `bf706bb` (9D), `694c39f` (9E), `2e7717d` (9F), `f457cdb` (9G), `93a45cf` (9H)
- **Removed 2026-10-10:** the live filter stack (9A–9C: the binding-owned stack, its caches, previews, presets and
  MCP tools), and with it the sections below on live filters at 24 MP (both depths, and the banded last stage) and
  on operations on a live-filtered layer, and every open item about a live filter. Gaussian Blur, Unsharp Mask,
  High Pass, Reduce Noise and Dust & Scratches remain as destructive filters on the same kernels
  (`core/src/filter_kernels.rs`). Retouching (9D–9I) is unchanged. An old `.rz` opens a live-filtered layer as the
  pixels it last composited. See [the removal ADR](convert-to-smart-object-status.md).

## Context
Phase 8 made layer content derive from retained originals ([phase-8-retained-source-status.md](phase-8-retained-source-status.md)).
Phase 9 adds non-destructive photographic filters that live on that binding (re-editable, reorderable, masked,
persisted) and upgrades retouching so it samples the right picture at native precision, never a display image.
It verified P7-06–P7-11 and P7-13–P7-18; P7-12 (cutout quality) remains open.

## Decisions — live filter stack (9A–9C)
- **The stack belongs to the retained layer binding** and preserves the source master and any RAW/local-mask or
  host-media attachments. Evaluation: source-profile conversion into the working profile → stack, first to
  last, **in source coordinates** → accumulated affine + fallback-depth conversion → layer mask, styles,
  compositing. A display preview or an already-filtered fallback is never a filter input; no canvas growth.
- **1–16 entries (`MAX_STORED_FILTERS`), five operators:** `gaussian_blur`, `unsharp_mask`, `high_pass` (sharing
  one Gaussian neighborhood, support `ceil(3σ)`, clamped edges, premultiplied convolution in the encoded
  working profile), `reduce_noise` and `dust_and_scratches` (each with its own neighborhood). An operator is a
  NEIGHBORHOOD + COMBINE; `live_filter_kernels.rs` dispatches both from one `Neighborhood` value so a cached
  plane and its key cannot drift. Radius is binary64, 0–64 untransformed source pixels.
- **One parameter table per operator** (`LiveFilterOp.parameters` in `app/Sources/LiveFilter.swift`) feeds the
  Add menu, descriptor legal keys, recorded steps and presets; a control sent to a non-owning operator is refused
  by name. Core `validate` checks control ownership both directions (`LiveFilter` is `deny_unknown_fields`, so a
  foreign control would make an older reader refuse the whole document rather than treat the stack as opaque).
- **Alpha laws:** Gaussian Blur replaces alpha; every other operator is **colour-only** — alpha passes through
  exactly and an alpha-0 sample is left untouched at every weight (hidden colour never surfaces). Sharpening
  clamps to [0,1] before blending. High Pass returns detail around a 0.5 pivot.
- **`reduce_noise`:** alpha-weighted guided filter (He/Sun/Tang) over a Rec. 709 opponent decomposition, box
  sums as two 1-D sliding passes so per-pixel cost is independent of radius. `luminance` (0–0.25) is a noise
  LEVEL, `color` (0–1) a chroma AMOUNT, `detail` (0–1) adds removed detail back. Cached plane is an opponent
  DELTA (inactive half is a bitwise identity); statistics are binary64; zero variance takes the mean (avoids 0/0
  NaN poisoning). Known trade-off: colour edges with no luminance step can bleed.
- **`dust_and_scratches`:** per-channel LOWER median over the lattice disc `dx²+dy² ≤ fl(r²)` (disc, not square,
  to avoid diagonal bias); alpha decides membership, not weight; `threshold` gates with strict `>`, so 0 is a
  plain median. One operator, not median + dust, so a threshold drag reuses the rank plane.
- **Per-entry blend:** each entry mixes over its own input with opacity × mask coverage through the precise
  alpha gate; Normal skips the blend table (reproduces 9A arithmetic). All layer modes except Dissolve (needs
  canvas coordinates) and Pass Through (group-only), which refuse.
- **One quantization:** processing depth = max(source, document); the whole chain runs in one straight f32
  plane quantized once after the last active filter. Entry-level exact bypasses: radius 0, opacity 0, disabled,
  Unsharp amount 0, dust radius < 1, denoise with both halves off. An unrecognized operator makes the **whole**
  stack opaque (cached rendering kept, semantic rerender refused, removal still allowed).
- **Per-entry native u16 coverage mask** that follows the filter's identity through reorder/insert/remove
  (fresh identity ⇒ full coverage; ids are minted fresh against the *saved* stack). Density/feather (0–256 px)
  follow layer-mask policy. Requests: ≤64 mask commands, 4,096 aggregate dabs, 256 KiB JSON per request.
- **Work/memory admission:** the forecast sums every active entry against the two-billion work-unit cap and
  refuses before allocation (12 MP at σ 6 already refuses); `reduce_noise` also carries a memory forecast
  (16 B/px plus the larger guided-filter half's row rings). Figures are `retained_source_render::work_plan` totals.
- **Persistence:** byte native **v23** / typed **v24** `LFLT` record (one descriptor + one mask ref per filter)
  in the shared binary pool; descriptor `schema_version` 2 (adds `opacity`, `blend_mode`; schema 1 still reads —
  Swift `LiveFilterEntry` has an explicit `init(from:)` for the defaults). **Adding an operator needs no version
  move**: older readers already meet unknown operators through the opaque-stack branch. Same-size source
  replacement keeps coverage; geometry-changing replacement with a filter mask refuses.
- **Cache:** three stages per binding under `process_cache_limit()` (working limit / 16) — gated prefix, unmasked kernel plane of the last active
  filter, masked native source — keyed exactly (never hashed) on source identity/revision, ICC bytes, depth,
  canonical JSON of applied entries, operator and kernel-relevant params. Amount/threshold/detail/colour/
  opacity/blend/mask drags reuse the kernel plane. Generations reject late inserts; cleared in `ImageDocument.close`.
- **Destructive twins** (Filters > Unsharp Mask…/High Pass…/Reduce Noise…/Dust & Scratches…, `apply_filter`
  kinds) share one body in `ops_photographic.rs` with the live dispatcher (never calls `mix`); selection-scoped,
  12× work budget, byte-only (typed destructive filtering came with 9F).
- **Tools/presets:** `get_live_filter`, `set_live_filter` (9A shape, optional `filter_index`), `set_live_filter_stack`,
  `add_live_filter`, `reorder_live_filters`, `remove_live_filter`, four `*_live_filter_preset` tools. A modal
  session records one canonical `set_live_filter_stack` with starting mask hashes and source dimensions. Presets
  (`DR_FILTER_PRESETS_DIR`, 128 × 8 KiB) store settings/order/opacity/blend/mask properties, never coverage;
  library writes are preferences, never Action steps; applying records the resolved stack.

## Rejected alternatives
- Convolution-family or bilateral denoise — cost grows with radius; the guided filter's does not.
- Separate `median` and `dust_and_scratches` operators — two cache identities for one rank plane.
- Bumping the native version or preset schema per operator — would make even all-Gaussian stacks opaque to older builds.
- Per-stage quantization — violates the single-quantization rule (held by construction/review, not by the 6-code tolerance).

## Consequences
- New operators: add a table row, own their neighborhood/cache key/forecast/bypasses, and never share a control
  name they do not own. Linear-light and float evaluation of the stack are not yet defined (refused).
- **Interactive ceilings.** Only the ⌘T PREVIEW of a live-filtered layer stays under `MAX_WORK_UNITS`: about 3.6 MP
  for `reduce_noise` and 3.8 / 3.1 MP for dust r1 / r2 at bicubic, the app default (Lanczos3 3.2, 3.4 / 2.8;
  `phase-9f-selected-tone-status.md`'s "Reduce Noise statistics in row rings" section). A non-enlarging,
  non-perspective preview whose commit plan admits resamples the layer's rendered pixels instead
  (`TransformPreviewSubstitute`, section "Operations on a live-filtered layer" below). An enlarging or perspective
  preview the cap refuses stays refused (a stated limit; the commit renders from the master). Replace Contents, RAW
  redevelop and Convert to Profile of a filtered layer are one-shot (`MAX_DESTRUCTIVE_WORK`). A transformed
  placement costs more at 1:1 (9B: bilinear ~10%, Lanczos3 ~50%).
- **Size ceilings.** The live edit itself is one-shot (`MAX_DESTRUCTIVE_WORK`), and the core plans admit a 24 MP
  `reduce_noise` at both depths (through its row rings). In the app the 16-bit one over a 24 MP 16-bit photo
  background is admitted at the 8 GiB a 16 GB Mac gets (`reduce_noise` has no band arm). A Gaussian-family or dust
  LAST stage over more than 8.4 MP runs banded: bytes fall from whole planes to a band, while Gaussian work rises
  slightly (the ring fill, 4 units per window pixel). The destructive twins are the full-size answer.

## Qualification
Bounds were fixed before observing output. One operator: **3 u16 codes / 1 byte code** vs a direct f64 oracle;
profile conversion + Gaussian **4 codes**; + bilinear affine **6 codes**; a chain of up to four operators
**6 codes**; destructive twin vs live at full coverage **1 byte code**. `dust_and_scratches` as first entry at full
weight is **exact**; near-tied rank fixtures are pinned by tolerance-free *membership* (every output is a value
some alpha-positive neighbour had). Oracles share no code with production (direct 2-D sums, over-large-square
disc scans, own Rec. 709 literals, re-derived alpha laws); ramp gates check window symmetry beyond the direct
oracle's radius band. Bypasses, outside-mask samples, original master/ICC, Undo/Redo, A→B→A and persistence
are exact checks. 9A additionally qualified RAW/local and host-frame preservation, actual UI on a real 2×
screen, fresh-process cache release on close and active cancellation. Probes:
[`scripts/live-filter-probe`](../scripts/live-filter-probe/README.md) (9A),
`scripts/filter-stack-probe/` (9B/9C) and `scripts/verify_phase9c_quality.py`, both since removed with live filters.

### 9C — noise cleanup
Photographic gate on two real checksum-pinned Phase 6 photographs plus `samples/portrait-exif6.jpg` (in fact the
synthetic plasma fractal with an EXIF tag) with frozen corruption and thresholds, compared at a *matched* Gaussian operating point: texture retained
0.950–0.991 (Gaussian 0.42–0.65); edge halo < 1 code with no gradient reversal on clean images; 97.8–100% of
struck dust pixels removed; zero rank samples outside their window. RAW noise controls alone do not close the
workflow: live cleanup works on non-RAW imports, respects masks exactly and composes distinctly with recipe NR.

### Live filters at 24 MP, both depths
**Decisions.** (1) The shared Gaussian neighbourhood (`sampling_gaussian`) keeps its ceil(3σ)
FIR below `LIVE_FAST_SIGMA` (4, equal to phase 1's `LEGACY_FAST_SIGMA` on purpose), its vertical pass reading a
ring of min(taps, h) rows instead of a whole plane (bit-identical, pinned in
`tests/live_gaussian_untruncated.rs`), and from σ4 up runs `sampling_gaussian_fast` (Deriche 4th order) as one
band. Every caller follows the switch: the live stack, the byte Unsharp Mask / High Pass, export output
sharpening, the 16-bit selected Photo filters and the export plan; Add Grain does not. (2) A live-filter edit
(`SetLiveFilter`) is a one-shot source edit: its stack and kernel ceilings are `MAX_DESTRUCTIVE_WORK`, and the
source edit takes `one_shot_work_limit` and `source_transform_scratch`'s 5/4. (3)
`live_filter_evaluate::plan` charges the running f32 plane once, and `work_plan_within` charges the stack phase
and the sampling phase as the larger, never the sum. A live-free plan is byte-identical to before.

**Live clause** (fixed before the run). Against a binary64 separable sampled Gaussian with ±ceil(8σ) taps
(untruncated), clamp-to-edge, on premultiplied unit scale, over four 512² fixtures at both depths' values and
σ ∈ {4, 5, 6, 8, 12, 16, 25, 40, 64}: max error ≤ 1/255 and mean ≤ 0.1/255 everywhere. **Passed** (worst max
0.0416/255, worst mean 0.0108/255 at σ64). The ceil(3σ) kernel itself would fail the mean at σ40 and σ64.

**Look change** (recorded, not a gate). From σ4 up every output changes slightly; below σ4 nothing does. At 8
bits Unsharp Mask and High Pass move by at most 1 level and a live Gaussian Blur by up to 3 (σ64); at 16 bits
by up to about 200 codes. A blurred edge's faint halo can reach slightly further, because pixels that were
fully transparent beyond the old 3σ support gain a few codes of alpha. No reader re-renders, so a reopened
`.rz` keeps its stored pixels until that layer's next re-render; Actions and export presets replaying Unsharp
Mask, High Pass or output sharpening at radius ≥ 4 produce the new output.

**Calibration and gates.** r16 stays the work unit (orchestrator ruling). Byte High Pass implies at most 34.6 units
per window pixel; × 1.15 = 39.8 ≤ `FAST_WORK_PER_PIXEL` (56). The `#[ignore]` probes time byte High Pass σ3.9 and
divide by the recorded R3.9/R16 = 2.86 (`tests/common/gaussian_reference.rs`). The legacy 16-bit `blur` σ4 row has
a thin margin. Footprint gate, fixed before the run: live, transform and sampling-dominant plans ≥ 1.25 × max E,
the preview and the selected edit ≥ 1.0×; all passed, and `live_filter_evaluate`'s module doc holds every row.
Tests: `tests/live_filter_admission.rs`, `tests/live_gaussian_untruncated.rs`. The app-drive table is in the
archived original; its refusals are taken up under
[Operations on a live-filtered layer](#operations-on-a-live-filtered-layer).

## 9D — native and transformed retouch
Clone/heal/spot/red-eye at both depths sample **Current**, **Current and Below** or **All Layers** (adjustments
above the target ignored) from an admitted core-built plate (`dr_doc_retouch_source`; host admission in
`DoctoredCore.retouchSource`), never `canvas.image`; typed gestures capture a native plate and transform it before a
separate display reduction. `source_transform` `[a,b,c,d,tx,ty]` maps destination centres to source centres;
byte/u16 share premultiplied bilinear taps with transparent out-of-bounds (hidden RGB never leaks); Aligned
strokes keep the matrix. `expand_destination` explicitly unions raster storage with the canvas. Healing
preserves destination alpha; New Retouch Layer heals a captured current-and-below backdrop and leaves the
original untouched; new output inside a gated group refuses. Native spot healing shares PatchMatch/pyramid/vote/
Poisson with f32 native-code RGB and one u16 writeback; native red-eye reads continuous RGB from the captured
document. Gesture recordings carry `footprint: "dabs"`; an explicit target-rasterization policy preserves an
artwork/master when sampling into a new layer. No native version change. Photographic spot gate (astronaut/fur):
10.8/9.1 byte-equivalent codes, 13.5%/8.4% of injected error (gates: <15 codes and <25%). Known limit: a 24-px
periodic hole has ambiguous texture phase — no unique-reconstruction claim beyond the 8-px exact gate. Receipt:
[phase-9d-synthesis-qualification.md](phase-9d-synthesis-qualification.md).

### 9D-1 — retouch sampling
Under a brightness adjustment, cloning with `sample: "all"` lands at 207 vs surroundings 175 (**31 codes** of
double application); `current_and_below` lands at 176 (**1 code**). Decisions: the cut is a **contribution gate,
not a visibility rewrite** (`Cut` in `core/src/style_composite.rs`, applied at three sites) because hiding a
clipped entry can flip its group from isolated to pass-through
(`group_tests::clipping_over_a_pass_through_group_isolates_it` vs
`retouch_sampling::cutting_a_clipped_member_does_not_re_isolate_its_group`). New-layer output runs the shipped
heal op on a one-layer temporary document built from the plate, because `heal_writeback::heal` reads the
destination in three roles (gate, contour term, lerp base) and strength lives only in its write weight.
Defaults stay per tool (`clone_stamp`/`heal_stroke`/`patch_region` composite; `spot_heal_stroke`/
`content_aware_fill` layer); `sample_all_layers` remains a legacy alias, refused only when it contradicts
`sample`. The sampled plate is tagged with the destination space (a sampled colour converts nowhere).

### 9E–9I (own ADRs)
[9E](phase-9e-content-aware-status.md) content-aware fill/Patch (P7-18); [9F](phase-9f-selected-tone-status.md)
selected native tone/filters and scoped statistics (P7-06/07); [9G](phase-9g-selections-masks-status.md) native
selections, masks, channels, auxiliary mattes (P7-08–11; native v25); [9H](phase-9h-paint-fill-status.md) native
paint/fill/pressure (P7-13/14); [9I](phase-9i-cutout-status.md) cutout research — this record's 9I paragraph
(RVM conditionally selected) is superseded: RVM was dropped and the gate rescoped as
[gate v2](phase-9i-gate-v2-status.md).

## Banded last stage
**Decision.** When the LAST active filter is Gaussian-family (`gaussian_blur`, `unsharp_mask`, `high_pass`) and its
whole f32 neighbourhood plane could never be cached — **16·s > `process_cache_limit()`** (working limit / 16:
128 MiB at the pin, 512 MiB at 8 GiB) — that stage runs in `BAND_ROWS` (128) row bands
(`core/src/live_filter_band.rs`): a ring of Win = min(h, 128 + 2H) straight input rows (H =
`sampling_gaussian::band_halo`: ceil(3σ) below σ4, ceil(4σ)+4 from σ4 up), one band of
`live_filter_kernels::neighborhood_band` output and the per-pixel combine straight into the native output. With a
single filter the rows come from the coloured source; under a stack, from the prefix plane. Nothing is published.
Denoise has no band arm and stays whole-plane, as do the destructive twins, export sharpening and the selected
filters (the rank arm came later; see below). `blur_bounded` is `blur_band(0..h)`, so there is one FIR. At
exactly 16·s == the limit (4096×2048) the plane is neither banded nor cached; the measured cacheability
boundary is 4096×2047.

**Plan** (`live_filter_evaluate::plan`; non-banded stacks unchanged). With the last stage L banded and prefix
P, d = depth/8, s = w·h: pre = [P ≠ ∅]·(16·s + max over P of kernel + mask); bytes = max(pre, [P ≠ ∅]·16·s +
band(L).bytes + 8·d·s) + (active + 2)·1 MiB; work = Σ_P (kernel + mask + combine) + band(L).work + 4·s under
`MAX_DESTRUCTIVE_WORK`. band(L) = Win·w·16 + the band pass + the mask's feather field + 2 MiB.

**Held by tests.** `tests/live_filter_banded.rs`: SHA-256 pins of nine edits at 4097×2049 captured on the
unchanged tree (`a77a225`), cancellation at both depths, the cache boundary. Footprint gate, declared before
measuring: live and transform plans ≥ 1.25 × max E, the preview ≥ 1.0×; the floors are ⌈1.25 × max E⌉ in
`tests/live_filter_admission.rs` and `tests/live_filter_45mp.rs`, and `live_filter_band`'s module doc holds
every run. The core plans admit an 8-bit 45 MP σ4 live edit, its preview and its source `transform` commit.

**Ceilings.** 16-bit placed images at 45 MP were capped by the native reopen envelope. Gate S, the measurement of
the old 8-bit native save estimate, was the evidence for replacing the estimate with the core's save plan
(the [phase-4 entry](phase-4-jobs-resource-status.md)). The footprint, gate S and app-drive tables are in the
archived original.

## Operations on a live-filtered layer

**Decision.** No work charge, host memory term or FFI changes; four plan rules and one preview stand-in.
- **Rank band arm** (`sampling_rank::{band_halo, band_plan, median_band}`; `median_bounded` is `median_band(0..h)`).
  A `dust_and_scratches` LAST stage above the cache limit runs through `live_filter_band` like the Gaussian
  family; each output gathers its own clamped disc, so any row range is bit-identical
  (`tests/live_filter_rank_band.rs`, pins taken at `23d7775`).
- **Byte one-shot ⌘T** (`doc_source_geometry`, `# Admission: the one-shot set transform`): since reversed. It
  charged a selected placed layer by its source plan's 5/4 and its held output instead of six copies of a raster
  its re-render never reads; the arm is now `pixels·6 + total·6 + canvas·8 + max_i S'_i + 4 MiB`, every selected
  raster and destination charged again.
- **Filtered Replace is one-shot** at both depths (`one_shot` also matches `Replace` when the rendered binding
  `live_filter_evaluate::has_active`): Replace Contents, linked refresh, host/RAW redevelop.
- **Profile conversion with a live filter is one-shot** at both depths (`live_filter_profile::plan`,
  `precision_color::color_plan`): `MAX_DESTRUCTIVE_WORK`, scratch `Σ O_i + max_i(S'_i − O_i)`. Live-free plans
  are byte-identical (`tests/filtered_source_ops_24mp.rs`).
- **⌘T preview stand-in** (`app/Sources/TransformPreviewSubstitute.swift`): when the true preview's plan
  throws, the request does not enlarge (larger singular value ≤ 1 + 1e-3, no quad) and the commit plan
  admits, `TransformMaskPreview` (8-bit) and `PrecisionGeometryGesture` (16-bit) preview the same transform
  of the document with each moving filtered placed layer rasterized (built once per owner, preview-only),
  planned and reserved as any raster preview, landing "Preview current (resampled from the filtered layer's
  pixels)". The commit is untouched. An enlarging or perspective request keeps the true error.

**Outcome.** At 24 MP every row is admitted in the app at both depths or is a stated ceiling. The ceilings (also
in the README):
- The 45 MP 8-bit ⌘T over a Background (G8-45) plans 545,330,335 + 4,261,671,840 = 4,807,002,175 bytes
  (`tests/live_filter_45mp.rs`), within a 16 GB Mac's 8 GiB working limit (open: the 100 MP case).
- An enlarging or perspective ⌘T preview the interactive cap refuses stays "Preview unavailable".
- A step refused only because undo history holds earlier states (the drive's sequence rows).

**The enlargement guard's record.** At ×0.9 the stand-in equals the true preview. After a committed ×0.5, a ×0.8
stand-in differs from it by a mean of at most 21 u16 codes (bound 257). At ×1.6, never substituted, it would
differ by a mean of 24–44 codes and up to about 18,000.

**Held by tests.** `tests/filtered_preview_stand_in.rs` (the premise at 2448×1632 and the fidelity figures). Gate
≥ 1.25 × max E; the rows are in `live_filter_band`'s `# Footprint`, `doc_source_geometry`'s
`# Admission: the one-shot set transform` and `live_filter_profile`'s `# One-shot`. The row, host-term, footprint,
ceiling, probe and drive tables are in the archived original.

## Open items (verified 2026-10-07)
- `DONE` — The 9A qualification drive is repaired: fixtures live in `~/devel/doctored-fixtures/phase9a`, the host stage is ported to the 9B stack API, and `verify_phase9a.py --mode all --require-current` passed (119 host groups, 30 MCP groups). The NEF hang was environmental; a `sample` tripwire (`nef-hang-sample.txt`) is in place.
- `TRACKED` — P7-12 gate half (advanced refinement brushing, colour decontamination; assisted session deferred) and therefore Phase 9 completion → carryover P7-12.
- `TRACKED` — Larger interactive filter/retouch workloads beyond the stated ceilings → P7-31 (§13A spatial neighborhoods), P7-33 (§13D).
- `TRACKED` — Live filters under linear-light evaluation → P7-24 (11E phase 2); on float documents → plan §11F open list.
- `DONE` — 9D-1's two confirmed review findings: plate host admission (`DoctoredCore.retouchSource` admits scratch and passes a real `RasterJobControl`) and the 16-bit ghost latch (`EditorViewController+Heal.strokeSourceImage` uses the native `precisionStroke.sourcePlate`) — `bf706bb`.
- `DONE` — Filter-preset library whole-library strictness (a newer preset broke list/apply/delete): now per-entry `broken` rows on the shared `PreferenceLibrary` (`app/Sources/LiveFilterPresetLibrary.swift`, `bc3c0fc`).
- `DONE` — Typed destructive spatial filters (9F, P7-06); P7-16/P7-17 typed retouch (9D).
- `DONE` — the core plans admit live Gaussian σ1/σ2/σ4, Unsharp Mask, High Pass and Dust & Scratches r2 at 24 MP at both depths, and the ⌘T commit of a σ4 layer; the live Gaussian family runs `sampling_gaussian_fast` from σ4 up (section above).
- `DONE` — Reduce Noise streams its guided-filter statistics through row rings (`sampling_box::BoxRows`), bit-identical; Dust & Scratches and Reduce Noise carry measured work charges (`phase-9f-selected-tone-status.md`, "Reduce Noise statistics in row rings").
- `DONE` — a Gaussian-family last stage above the cache limit runs in row bands, bit-identical, so the core plans admit an 8-bit 45 MP σ4 live edit, its preview and its source `transform` commit (section above; `23d7775`).
- `DONE` — Dust & Scratches as a banded last stage, the byte one-shot ⌘T plan without the never-read raster, one-shot filtered Replace and profile conversion, and the ⌘T preview stand-in (section above; `45c97f9`). Every row is admitted in the app or a stated ceiling.
- `DROPPED` — Black-box proof that Normal reproduces 9A arithmetic via `blend_of → None`: accepted as unobservable; confirmed by code reading against `c725d88` (original, "residual gap").
- `DROPPED` — Making `sample: "all"` free on byte documents by reusing the held projection: review suggestion only; 9D chose admitted plates.
- `DROPPED` — a Denoise (`reduce_noise`) band arm, left open by the banded last stage and the operations on a live-filtered layer: not built, dropped (`62930ad`) once its 16-bit 24 MP case was admitted under the RAM-scaled working limit.
- `DROPPED` — banded prefix stages, left open by the banded last stage: a stack of several live filters at 45 MP plans its prefix whole-plane because only the last stage runs in bands.
- `OPEN` — the enlarging or perspective ⌘T preview of a live-filtered placed layer, left open by the ⌘T preview stand-in: it stays "Preview unavailable" when the interactive cap refuses.
- `OPEN` — left: the 8-bit placed ⌘T at 100 MP plans about 10.6 GB, over a 16 GB Mac's 8 GiB; re-fitting the byte arm needs a decision.
- `OPEN` — working-memory estimate follow-ups, including the legacy 16-bit `blur` σ4 work margin.
