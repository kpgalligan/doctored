# Phase 9 — editable photographic filters and retouching (ADR)

- **Status:** Accepted — open items remain (9A–9H closed within scope; 9I / P7-12 gate half open)
- **Phase / dates:** 9A–9D recorded here, 9E–9I summarized with their own ADRs; 2026-09-11 → 2026-09-12
- **Original:** [archive/phase-9-live-filters-status.md](../archive/phase-9-live-filters-status.md) — full history, measurements, receipts, review findings
- **Key commits:** `c725d88` (9A), `33855da` (9B), `decada4` (9C), `01a5e51` (9D-1), `bf706bb` (9D), `694c39f` (9E), `2e7717d` (9F), `f457cdb` (9G), `93a45cf` (9H)

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
- **Cache:** three stages per binding under 128 MiB — gated prefix, unmasked kernel plane of the last active
  filter, masked native source — keyed exactly (never hashed) on source identity/revision, ICC bytes, depth,
  canonical JSON of applied entries, operator and kernel-relevant params. Amount/threshold/detail/colour/
  opacity/blend/mask drags reuse the kernel plane. Generations reject late inserts; cleared in `ImageDocument.close`.
- **Destructive twins** (Filters > Unsharp Mask…/High Pass…/Reduce Noise…/Dust & Scratches…, `apply_filter`
  kinds) share one body in `ops_photographic.rs` with the live dispatcher (never calls `mix`); selection-scoped,
  12× work budget, byte-only (typed destructive filtering came with 9F).
- **Tools/presets:** `get_live_filter`, `set_live_filter` (9A shape, optional `filter_index`), `set_live_filter_stack`,
  `add_live_filter`, `reorder_live_filters`, `remove_live_filter`, four `*_live_filter_preset` tools. A modal
  session records one canonical `set_live_filter_stack` with starting mask hashes and source dimensions. Presets
  (`RZ_FILTER_PRESETS_DIR`, 128 × 8 KiB) store settings/order/opacity/blend/mask properties, never coverage;
  library writes are preferences, never Action steps; applying records the resolved stack.

## Rejected alternatives
- Convolution-family or bilateral denoise — cost grows with radius; the guided filter's does not.
- Separate `median` and `dust_and_scratches` operators — two cache identities for one rank plane.
- Bumping the native version or preset schema per operator — would make even all-Gaussian stacks opaque to older builds.
- Per-stage quantization — violates the single-quantization rule (held by construction/review, not by the 6-code tolerance).

## Consequences
- New operators: add a table row, own their neighborhood/cache key/forecast/bypasses, and never share a control
  name they do not own. Linear-light and float evaluation of the stack are not yet defined (refused).
- Interactive ceilings. Only the ⌘T PREVIEW of a live-filtered layer stays under `MAX_WORK_UNITS`; the
  measured row-ring and rank charges lowered it to about 3.6 MP for `reduce_noise` and 3.8/3.1 MP for dust r1/r2
  (bicubic, the app default; Lanczos3 3.2, 3.4/2.8; `phase-9f-selected-tone-status.md`'s "Reduce Noise statistics
  in row rings" section), and that cap is unchanged. The preview of a non-enlarging, non-perspective transform whose
  commit plan admits resamples the layer's rendered pixels instead (`TransformPreviewSubstitute`, section
  "Operations on a live-filtered layer" below): the probe lands it at 4, 8 and 12 MP at both depths. Replace
  Contents, RAW redevelop and Convert to Profile of a filtered layer are one-shot (`MAX_DESTRUCTIVE_WORK`), admitted at
  4–12 MP and 24 MP with no charge lowered. Still refused: an enlarging or perspective ⌘T preview the cap
  refuses (a stated limit; the commit renders from the master). A transformed placement costs more at 1:1
  (9B: bilinear ~10%, Lanczos3 ~50%). The live edit itself is one-shot (`MAX_DESTRUCTIVE_WORK`, Part B),
  and its statistics are row rings, so the core plans admit a 24 MP `reduce_noise` at both
  depths; in the app the 16-bit one is refused over a 24 MP 16-bit photo background by 62 KB of host
  admission at the 2 GiB reference limit (a stated
  ceiling: Denoise has no band arm); at the 8 GiB limit a 16 GB Mac gets it is admitted (batch 163: plan
  1,473,377,920, max E 935,445,176, 1.575×), and its 2,147,545,848 B fits an 8 GB Mac's 2,862,612,480. The destructive twins are the full-size answer. Since Part E a Gaussian-family LAST stage over
  more than 8.4 MP (16·s above the 128 MiB cache limit) runs banded, and so does dust: bytes fall
  from whole input and kernel planes to a band, while Gaussian work rises slightly (the ring fill, 4 units
  per window pixel — +7% on the fast arm — and, below σ4, the FIR's per-band halo rows, ≈ +11% at σ3.9);
  the rank work is unchanged. `reduce_noise` has no band arm.

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
[`scripts/filter-stack-probe`](../scripts/filter-stack-probe/README.md) (9B/9C), `scripts/verify_phase9c_quality.py`.

### 9C — noise cleanup
Photographic gate on two real checksum-pinned Phase 6 photographs plus `samples/portrait-exif6.jpg` (in fact the
synthetic plasma fractal with an EXIF tag) with frozen corruption and thresholds, compared at a *matched* Gaussian operating point: texture retained
0.950–0.991 (Gaussian 0.42–0.65); edge halo < 1 code with no gradient reversal on clean images; 97.8–100% of
struck dust pixels removed; zero rank samples outside their window. RAW noise controls alone do not close the
workflow: live cleanup works on non-RAW imports, respects masks exactly and composes distinctly with recipe NR.

### Part B + Part D phase 2 — live filters at 24 MP, both depths
**Decisions.** (1) The shared Gaussian
neighbourhood (`sampling_gaussian`) keeps its ceil(3σ) FIR below `LIVE_FAST_SIGMA` (4, equal to phase 1's
`LEGACY_FAST_SIGMA` on purpose), its vertical pass now reading a ring of min(taps, h) horizontal rows instead
of a whole horizontal plane — bit-identical, pinned by SHA-256 against the pre-change tree
(`tests/live_gaussian_untruncated.rs`) — and from σ4 up runs `sampling_gaussian_fast` (Deriche 4th order) as
one band. Every caller follows the switch: the live stack, the byte Unsharp Mask / High Pass, export output
sharpening, the 16-bit selected Photo filters and the export plan; Add Grain does not. (2) A live-filter edit
(`SetLiveFilter`) is a one-shot source edit: its stack and kernel ceilings are `MAX_DESTRUCTIVE_WORK`, the
source edit takes `one_shot_work_limit` and `source_transform_scratch`'s 5/4; the ⌘T preview, Replace, RAW
redevelopment and the profile conversions keep `MAX_WORK_UNITS`. (3) `live_filter_evaluate::plan` charges
the running f32 plane once (HEAD 32 B/px; it is the published prefix plane), and `work_plan_within`
charges the stack phase (coloured source + stack) and the sampling phase (input, output, tiling, plus under
a stack the filtered plane and `published_bytes`) as the larger, never the sum. A live-free plan is
byte-identical to HEAD.

**Live clause** (fixed before the run). G′ = {4,5,6,8,12,16,25,40,64}; F1 quadrant, F2 lines, F3 noise, F4
50%-alpha disc (phase 1's generators), 512², at 16-bit values and 8-bit values ×257, as a 16-bit retained
source in a 16-bit sRGB document, live `gaussian_blur`. R∞ = binary64 separable sampled Gaussian, ±ceil(8σ)
taps, normalized, clamp-to-edge, premultiplied unit scale. Pass: E ≤ 1/255 and M ≤ 0.1/255 everywhere.
**Passed**: worst E 0.0391–0.0416/255, worst M 0.0018/255 (σ8) to 0.0108/255 (σ64). Information rows, HEAD's
ceil(3σ) kernel on the same metric: E 0.44 (σ4) → 0.67/255 (σ64), M 0.018 (σ4), 0.076 (σ25), 0.121 (σ40),
0.188/255 (σ64) — the truncated kernel itself would fail M at σ40/64.

**Look change** (HEAD vs new output codes at the output depth, worst over F1–F4, σ ∈ {4, 8, 25, 64}; recorded,
not a gate). 8 bits — live (byte master in a byte document), byte `rz_image_filter_op`, and so export
sharpening at 8 bits, identical rows: Unsharp Mask a1 max 1 level, mean ≤ 0.026; a5 max 1, mean ≤ 0.058;
High Pass max 1, mean ≤ 0.22; live Gaussian Blur max 1 (σ4/8), 2 (σ25), 3 (σ64), mean ≤ 0.19. 16 bits — live and
the 16-bit selected filter over a full selection, identical to ±0.001 mean: Unsharp Mask a1 max 34/39/34/62 codes
(σ4/8/25/64), mean ≤ 6.5; a5 max 170/195/165/203, mean ≤ 14.9; High Pass max 107/136/158/165, mean ≤ 45.7;
live Gaussian Blur premultiplied max 106/136/158/165, mean ≤ 45.6 — its straight colour differs by up to the
full range only where HEAD's alpha was 0 beyond the 3σ support and the untruncated tail now leaves a few codes
of alpha (F4); so a blurred edge's faint halo can extend slightly further than before. Below σ4 nothing changes. Saved files: no reader re-renders (`render8`/`render16` run only
for edits), so a reopened `.rz` keeps its stored pixels and the new look appears in the live preview and at
that layer's next re-render; Actions and export presets replaying Unsharp Mask, High Pass or output
sharpening at radius ≥ 4 produce the new output.

**Calibration** (orchestrator ruling `04b`: r16 stays the work unit). R16 on the 10c8007 build in the same
session 1.1903e9 units/s (phase-1 record 1.1734e9, +1.4%, within the 10% gate). Byte High Pass on the
changed build at σ4/12/25/64: 34.6 / 30.1 / 26.4 / 22.8 units per window pixel (largest of three runs);
34.6 × 1.15 = 39.8 ≤ `FAST_WORK_PER_PIXEL` (56). The `#[ignore]` probes now time byte High Pass σ3.9 and
divide by `tests/common/gaussian_reference.rs`'s recorded R3.9/R16 = 2.86 (the row ring halved the σ3.9
FIR's time: at 10c8007 the ratio was 1.41). The legacy 16-bit `blur` edit's σ4 row, unchanged code, read
46.7–50.2 in this session against phase 1's 48.4 (× 1.15 up to 57.7), a thin margin.

**Footprint** (`live_filter_evaluate`'s module doc holds every row; one case per process, 3 runs,
`MallocLargeCache=0`, baseline twins E 0). Gate fixed before the run: live, transform and sampling-dominant
cases ≥ 1.25 × max E, the preview and the selected edit ≥ 1.0×. All passed: 16-bit live 1.508–1.546×,
8-bit live 1.264–1.298×, ⌘T commits 1.538× / 1.290×, 3000×2000 ×2 placements 1.526–2.303×, P16 1.234×,
S16 1.071×. The 24 MP 16-bit σ4 plan is 1.479e9 scratch over 0.384e9 retained (HEAD 2.30e9 scratch,
refused); since Part E's banded last stage, 0.967e9 (section below).

**App drive** (`make app`, MCP on an own background instance; 6000×4000 `new_document` plus a placed
6000×4000 PNG; durations in brackets). These rows are the app's behaviour; the core plans above are not.

| step | 16-bit | 8-bit |
|---|---|---|
| live `gaussian_blur` σ4 | admitted (1.6 s) | admitted (1.3 s) |
| re-set to σ12 | refused by the host's working-memory admission | admitted (1.6 s) |
| stack [σ4, Unsharp Mask σ1.5, Dust & Scratches r2] | refused by the host's working-memory admission | admitted (5.8 s) |
| `transform_layer` ×0.9 Lanczos (⌘T) | refused by the core geometry plan, 2,159,050,942 B with the host's retained bytes | refused by the byte geometry plan, 2.61 GB |

Each refused row was refused at HEAD too. The core's own plans for the 16-bit re-edit and the ⌘T commit fit
the working limit without the host's working set (1.87e9 and 1.86e9 bytes). **Superseded in part by Part E's drive**
(section below): the app now admits the 16-bit σ12 re-edit and the 16-bit ⌘T; the 16-bit stack and the
8-bit ⌘T still refuse. Tests: `tests/live_filter_admission.rs` (24 MP plans at both depths with these floors, the warm
re-edit, the ⌘T commit and the still-refused preview; warm cache planes charged as retained),
`tests/live_gaussian_untruncated.rs` (pins, the clause on 131×300 fixtures, a precancelled σ12 edit).

## 9D — native and transformed retouch
Clone/heal/spot/red-eye at both depths sample **Current**, **Current and Below** or **All Layers** (adjustments
above the target ignored) from an admitted core-built plate (`rz_doc_retouch_source`; host admission in
`RasterCore.retouchSource`), never `canvas.image`; typed gestures capture a native plate and transform it before a
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

## Part E — banded last stage
**Decision.** When the LAST active filter is
Gaussian-family (`gaussian_blur`, `unsharp_mask`, `high_pass`) and its whole f32 neighbourhood plane could
never be cached — **16·s > `LIVE_FILTER_CACHE_LIMIT`** (128 MiB, i.e. above 8,388,608 pixels) — that stage
runs in `BAND_ROWS` (128) row bands (`core/src/live_filter_band.rs`): a ring of Win = min(h, 128 + 2H)
straight input rows (H = `sampling_gaussian::band_halo`: ceil(3σ) for the FIR below σ4, the fast filter's
ceil(4σ)+4 from σ4 up), one band of `live_filter_kernels::neighborhood_band` output and the per-pixel combine
straight into the native output. With a single filter the rows come straight from the coloured source (no
straight plane); under a stack, from the prefix plane. There is no whole input plane and no whole kernel
plane, and nothing is published (the cache could not have kept the plane). Rank and Denoise have no band arm
and stay whole-plane, as do the destructive twins, export sharpening and the selected filters. `blur_bounded`
is `blur_band(0..h)`, so there is still one FIR. At exactly 16·s == the limit (4096×2048) the plane is
neither banded nor cached — the store counts the entry's key and profile bytes beside its pixels — a
one-row gap that only runs the whole-plane path where nothing would be kept; the measured cacheability
boundary is 4096×2047.

**Plan** (`live_filter_evaluate::plan`; non-banded stacks byte-identical to Part B). With the last stage L
banded and prefix P, d = depth/8, s = w·h: pre = [P ≠ ∅]·(16·s + max over P of kernel + mask); bytes =
max(pre, [P ≠ ∅]·16·s + band(L).bytes + 8·d·s) + (active + 2)·1 MiB (the 8·d·s is the native output and the
copy its tiling makes); work = Σ_P (kernel + mask + combine) + band(L).work + 4·s under the same
`MAX_DESTRUCTIVE_WORK`. band(L) = Win·w·16 (input ring) + the band pass + the mask's feather field + 2 MiB;
its work is the banded pass over the whole plane (FIR: w·(window rows + h)·4·taps + 12·s; fast:
`sampling_gaussian_fast::plan` at 128 rows), the ring fill 4 units per window pixel, the mask and the combine.

**Pins.** `tests/live_filter_banded.rs` (a): SHA-256 of nine edits at 4097×2049 (splitmix noise, a
50%-alpha disc), captured on the unchanged tree a77a225 — 8-bit σ2 / σ4 / σ12, a feathered Unsharp
(opacity 0.6, Luminosity), Unsharp → σ4; 16-bit σ3.9, High Pass σ3, Unsharp → σ4 and Dust r2 (control) —
all bit-identical on the banded build. (b) cancellation at both depths, (c) the boundary through the kernel
cache, `tests/live_filter_45mp.rs` (d)/(e).

**Footprint** (`live_filter_band`'s module doc holds every run; method as Part B; banded build;
baselines E 0; gate declared before measuring: L and T ≥ 1.25 × max E, P ≥ 1.0×; nothing tuned). 24 MP cases
are Part B's own, 45 MP cases an 8256×5504 opaque plasma sRGB byte master in an sRGB byte document.

| case | max E | retained | scratch | scratch / max E | margin |
|---|---|---|---|---|---|
| L16-4 σ4 | 383,680,800 | 384,026,951 | 967,288,365 | 2.521 | 796,168,332 |
| L8-4 σ4 | 233,144,464 | 192,026,911 | 486,555,442 | 2.087 | 1,468,901,295 |
| L16-2 σ2 | 383,615,264 | 384,026,951 | 967,288,365 | 2.522 | 796,168,332 |
| L8-2 σ2 | 217,284,752 | 192,026,911 | 486,555,442 | 2.239 | 1,468,901,295 |
| L16-S unsharp σ1.5 → σ12 | 772,276,776 | 384,027,266 | 1,294,186,285 | 1.676 | 469,270,097 |
| L16-R σ4 → σ12, warm | 240,681,152 | 384,027,572 | 967,288,365 | 4.019 | 796,167,711 |
| L8-R the same | 243,237,032 | 192,027,564 | 486,555,442 | 2.000 | 1,468,900,642 |
| T16 source `transform` ×0.9 Lanczos3 of a σ4 layer | 540,967,320 | 384,026,303 | 876,088,365 | 1.620 | 887,368,980 |
| T8 the same | 251,854,992 | 192,026,295 | 440,955,442 | 1.751 | 1,514,501,911 |
| P16 preview, divisor 4 | 230,277,312 | 384,026,303 | 640,866,724 | 2.783 | 1,122,590,621 |
| L8-45 σ4 | 544,965,016 | 181,781,561 | 914,065,202 | 1.677 | 1,051,636,885 |
| T8-45 source `transform` of a σ4 layer | 658,342,368 | 363,550,817 | 827,836,242 | 1.257 | 956,096,589 |
| P8-45 preview, divisor 6 | 544,670,104 | 545,314,913 | 806,521,088 | 1.481 | 795,647,647 |

Ratios above 2.0 are dominated by `work_plan_within`'s affine sampling phase (≈ 16 B/px), which Part E does
not touch; the banded stack phase plans ≈ 0.44e9 / 0.25e9 at 24 MP. At a77a225 the L8-45 core plan was
181,781,561 + 2,317,503,282 = 2,499,284,843 B and refused. The test floors are ⌈1.25 × max E⌉
(`tests/live_filter_admission.rs`, `tests/live_filter_45mp.rs`). The remaining 45 MP core ceiling is a
STACK, whose prefix still runs whole-plane: Unsharp σ1.5 → σ4 plans 181,781,876 + 2,059,771,552 B and
refuses. 16-bit at 45 MP is out of reach here at the 2 GiB reference limit: 16-bit placed images are capped by
the native reopen envelope.

**Gate S — the 8-bit native save estimate** (measured before any code change). The app's
estimate is `referenced_bytes·4 + 64 MiB` (`CapturedSave.swift`, `EditingStateArchive.swift`), admitted "on its own" against 2 GiB with the
document's own retained bytes. The candidate R·k + 64 MiB, R = `referenced_bytes −
categories.contiguous_views.referenced_bytes`, had to reach 1.25 × max E in every case and view state and
admit S1 for some k ∈ {2, 3, 4}. E of `rz_doc_save_native` at a77a225, method as above; S1 = File ▸ New's
8256×5504 white Background + a placed 8256×5504 noise layer; S2 one plain 8256×5504 noise raster; S2p the
placed layer alone; S3 two 7500×7500 noise layers. (R·k + 64 MiB) / max E:

| case | views | max E | R | HEAD need (est + own) | k2 | k3 | k4 |
|---|---|---|---|---|---|---|---|
| S1 | live | 744,571,456 | 727,109,708 | 4,793,212,122 (refused) | 2.04 | 3.02 | 4.00 |
| S1 | evicted | 744,555,072 | 727,109,708 | 3,339,099,354 (refused) | 2.04 | 3.02 | 4.00 |
| S2 | live | 561,873,328 | 181,779,747 | 1,703,063,983 | **0.77** | **1.09** | 1.41 |
| S2 | evicted | 561,856,944 | 181,779,747 | 976,007,599 | **0.77** | **1.09** | 1.41 |
| S2p | live | 561,791,432 | 545,338,818 | 3,157,301,288 (refused) | 2.06 | 3.03 | 4.00 |
| S2p | evicted | 743,670,360 | 545,338,818 | 2,430,244,904 (refused) | 1.56 | 2.29 | 3.02 |
| S3 | live | 920,584,888 | 450,025,708 | 4,117,237,404 (refused) | **1.05** | 1.54 | 2.03 |
| S3 | evicted | 920,601,296 | 450,025,708 | 2,317,237,404 (refused) | **1.05** | 1.54 | 2.03 |

No k passed: k4 is the first to clear 1.25× everywhere, and S1 then needs 3,339,099,354 B. E per R-byte varies
about 3× (≈ 3.09 for one incompressible plane, ≈ 1.02 for S1, whose Background compresses), so an honest
bound needs a per-plane term the report does not return. Eviction left E unchanged
for S1, S2 and S3 (< 33 KB), so their view bytes are not save scratch; for S2p E rose by 181,878,928 B (one
4·s plane) after eviction. The save estimate is unchanged in this batch. Its need is 4·referenced + 64 MiB + the
document's own retained bytes, so by arithmetic on the gate S per-pixel figures (not driven at these sizes)
the need for an 8-bit Background plus a same-size placed layer reaches the 2 GiB limit at about 20 MP (views
cached: referenced 24, own 8 B/px, so 4·24 + 8 = 104 B/px) and 29 MP (evicted: referenced 16, own 8 B/px,
72 B/px); with a live σ4 executed (S1 re-run) the evicted own bytes are 12 B/px (76 B/px), about 27.4 MP. The only app figure, the 45 MP drive's refusal below
(3.28 GB), is that evicted live-filter need: 4 × 727,109,353 + 67,108,864 + 545,321,207 = 3,520,867,483 B
(3.28 GiB), not the cached-views one.
Fixed since: the core's save plan replaced this estimate
(the [phase-4 entry](phase-4-jobs-resource-status.md)); the two save refusals below are
the a77a225-era rows.

**App drive** (`make app`, MCP on an own background instance; durations in brackets; these rows are the app's
behaviour, not the core plans). 45 MP 8-bit: `new_document` 8256×5504 and `place_image` of an 8256×5504 RGB8
PNG.

| step | outcome |
|---|---|
| `set_live_filter` σ4, `get_live_filter` | admitted (2.8 s), σ4 present (refused at a77a225) |
| `transform_layer` ×0.9 `lanczos` (⌘T) | refused: "This job needs 3.81 GB of working memory on its own…" — the 8-bit ⌘T admits the byte geometry plan (`rz_doc_geometry_edit_plan`, 4,094,794,240 B here), not the source `transform` measured as T8-45 |
| `undo`, `redo` | admitted (0.6 s each; they undo and redo the live filter) |
| `save_copy` `.rz` with the Background | refused: "This job needs 3.28 GB of working memory on its own…" |
| `delete_layer` Background, then `save_copy` `.rz` | refused: "This job needs 2.43 GB of working memory on its own…"; no file, so no reopen |

24 MP (6000×4000 `new_document` plus a placed 6000×4000 PNG):

| step | 16-bit | 8-bit |
|---|---|---|
| live `gaussian_blur` σ4 | admitted (1.6–1.7 s) | admitted (1.4 s) |
| `transform_layer` ×0.9 `lanczos` after σ4 | **admitted (5.4 s)** — refused in Part B | refused: "2.02 GB on its own" (Part B: 2.61 GB) |
| re-set to σ12 | **admitted (5.6 s)** — refused in Part B | admitted (1.5 s) |
| stack [σ4, Unsharp Mask σ1.5, Dust & Scratches r2] | refused by the host's working-memory admission | admitted (5.8 s) |
| `transform_layer` after that step | admitted (4.8 s; the layer still carried σ12) | refused: "2.61 GB on its own" |

## Operations on a live-filtered layer

**Decision.** No work charge, host memory term or FFI changes; four plan rules and one preview stand-in.
- **Rank band arm** (`sampling_rank::{band_halo, band_plan, median_band}`; `median_bounded` is `median_band(0..h)`).
  A `dust_and_scratches` LAST stage above the cache limit runs through `live_filter_band` like the Gaussian
  family (Part E); each output gathers its own clamped disc, so any row range is bit-identical. Pins:
  `tests/live_filter_rank_band.rs` (a), SHA-256 taken at 23d7775 — 8/16-bit dust r2 t0, dust r4 t10 under a
  feathered mask (feather 8, opacity 0.6, Luminosity), 16-bit [unsharp σ1.5 → dust r2], 8-bit [σ4, unsharp
  σ1.5, dust r2] at 4097×2049; the 16-bit dust r2 digest equals Part E's own pin.
- **Byte one-shot ⌘T** (`doc_source_geometry`'s third `# Admission`): a commit charges a selected placed
  layer by its source plan's 5/4 and its held output, not by six copies of a raster its re-render never reads;
  set members are sequential with earlier outputs held (`Σ O_i + max_i(S'_i − O_i)`).
- **Filtered Replace is one-shot** at both depths (`one_shot` also matches `Replace` when the rendered binding
  `live_filter_evaluate::has_active`): Replace Contents, linked refresh, host/RAW redevelop.
- **Profile conversion with a live filter is one-shot** at both depths (`live_filter_profile::plan`,
  `precision_color::color_plan`): `MAX_DESTRUCTIVE_WORK`, each source S'_i, held outputs O_i, scratch
  `Σ O_i + max_i(S'_i − O_i)` (the typed plan summed every layer). Live-free plans are byte-identical (the
  23d7775 constants in `tests/filtered_source_ops_24mp.rs`).
- **⌘T preview stand-in** (`app/Sources/TransformPreviewSubstitute.swift`): when the true preview's plan
  throws, the request does not enlarge (larger singular value ≤ 1 + 1e-3, no quad) and the commit plan
  admits, `TransformMaskPreview` (8-bit) and `PrecisionGeometryGesture` (16-bit) preview the same transform
  of the document with each moving filtered placed layer rasterized (built once per owner, preview-only),
  planned and reserved as any raster preview, landing "Preview current (resampled from the filtered layer's
  pixels)". The commit is untouched. An enlarging or perspective request keeps the true error.

**Rows** (24 MP = 6000×4000; final drive below):

| row | 8-bit | 16-bit |
|---|---|---|
| A1 σ4 → σ12 re-edit | admitted | admitted (Part E) |
| A2 stack [σ4, USM σ1.5, dust r2] | admitted | **admitted** (Rank band; was host-refused) |
| A3 ⌘T ×0.9 Lanczos3 (commit), A3-0 unfiltered | **admitted** (was 2.02 GB / host-refused) | admitted |
| A4-W live `reduce_noise`, white background | admitted | admitted |
| A4-P the same over an opened 24 MP photo | admitted | **ceiling** (host admission, below) |
| B1 Replace Contents | **admitted** (was work cap) | **admitted** |
| B2 RAW redevelop | refused at the 2 GiB reference by the measured RAW decode charge (4.31 GB at 24 MP); admitted at 8 GiB, B2-0 too | same |
| B3 Convert to Profile | **admitted** | **admitted** |
| B4 Assign | refuses a source by name; `rasterize_sources` admitted | same |
| I1 ⌘T preview, 4–12 MP | stand-in (probe below) | same |
| I2 Replace / RAW / Convert, 4–12 MP | admitted ((i); D1–D3 in the app) | same |
| 45 MP ⌘T over a Background (addendum 2) | **ceiling**: 2,287,452,192 B scratch alone (was 4,094,794,240) | — |

**Host-term record** (own instance, `MallocLargeCache=0`, `resource_usage` + `/usr/bin/footprint` before
every measured call). A4-P-16, identical on the Phase-0 build (23d7775) and the final one: unique retained
674,167,928 B (`precision_pixels` 576,000,000 — background, master and fallback — plus 96,009,384 display
`image_pixels` and a 2,135,792 B capture payload), history baseline 386,155,312 (one undo step), cache 0,
transient 0, phys_footprint 436,127,808 before and 436,095,040 after the refusal. The core plan for
`reduce_noise` on that layer is 1,473,377,920 B of scratch (retained 384,020,100 in the core's own count),
so scratch + the host's retained union is 2,147,545,848 B, 62,200 B over 2 GiB: the host admission refuses
("…no other document is open to close. Use a smaller image, or lower its depth first with Image ▸ Mode ▸
Convert Precision…" since this batch's `LiveFilterWork` remedy). Denoise has no band arm (out of scope:
a band would need per-band halo recompute and re-seeded rings in `guided_bounded`, a new measured charge).

**Footprint** (method as Part B; cold cache; master 6000×4000 plasma Display P3 in an sRGB document, "+W" =
placed over a white 6000×4000 layer; every baseline twin E 0; gate ≥ 1.25 × max E, nothing tuned; the full
rows are `live_filter_band`'s `# Footprint`, `doc_source_geometry`'s third `# Admission` and
`live_filter_profile`'s `# One-shot`):

| case | max E | retained | scratch | scratch / max E | margin |
|---|---|---|---|---|---|
| K16-D live dust r2 | 287,260,864 | 384,026,947 | 967,288,365 | 3.367 | 796,168,336 |
| K8-D | 216,531,112 | 192,026,907 | 486,555,442 | 2.247 | 1,468,901,299 |
| K16-S the A2 stack | 788,513,344 | 384,027,835 | 1,250,285,170 | 1.586 | 513,170,643 |
| K8-S | 881,197,704 | 192,027,795 | 1,121,262,642 | 1.272 | 834,193,211 |
| G8 +W ⌘T, σ4 | 252,362,920 | 288,029,863 | 1,213,147,904 | 4.807 | 646,305,881 |
| G8-0 no filter | 252,018,856 | 288,030,186 | 1,093,147,904 | 4.338 | 766,305,558 |
| G8-2L σ4 + unfiltered placed, both moving | 425,525,560 | 480,037,536 | 1,290,907,904 | 3.034 | 376,538,208 |
| G16 typed `affine_set`, σ4 | 541,393,256 | 385,254,767 | 881,336,205 | 1.628 | 880,892,676 |
| R16 Replace, σ4 | 384,991,520 | 577,258,601 | 967,288,365 | 2.512 | 602,936,682 |
| R8 | 576,160,152 | 480,033,697 | 1,087,292,047 | 1.887 | 580,157,904 |
| RN16 Replace, `reduce_noise` | 962,069,176 | 577,258,860 | 1,475,423,405 | 1.534 | 94,801,383 |
| RN8 | 1,153,598,280 | 480,033,956 | 1,595,427,087 | 1.383 | 72,022,605 |
| C16 Convert to Display P3, σ4 | 230,015,144 | 385,254,767 | 1,160,976,420 | 5.047 | 601,252,461 |
| C8 | 233,029,824 | 288,038,973 | 585,440,130 | 2.512 | 1,274,004,545 |
| C8-2L two placed 6000×4000 (σ4, unfiltered) | 287,523,032 | 480,046,646 | 681,441,602 | 2.370 | 985,995,400 |
| C16-2L two placed 4000×3000 (σ4, unfiltered) | 384,778,528 | 385,256,688 | 776,976,420 | 2.019 | 985,250,540 |

Above 2.0: G* by the unselected layer's ×6 and the canvas ×8 (out of scope), K*-D by the
affine sampling phase, R16/C* by a source's 5/4 sampling phase. RN* plans are admitted at 24 MP by the core
(margins 95 / 72 MB), so they are not stated ceilings; in the app the host's retained union decides. G8-45
(8256×5504, addendum 2): the revised plan (545,330,327 + 2,287,452,192) still exceeds 2 GiB, so it is a
stated ceiling and was not executed.

**Ceilings at 4–12 MP** (`tests/filtered_interactive_ceilings.rs` (h) and (i); ×0.9 about the centre;
r + s = retained + scratch; the true preview refuses by the interactive work cap in every cell, both
samplers, which is the condition the stand-in serves):

| size | depth | filter | commit r + s, work (bicubic) | stand-in r + s | Replace r + s, work | Convert r + s |
|---|---|---|---|---|---|---|
| 2448×1632 | 16 | reduce noise | 0.40e9, 2.24e9 | 0.12e9 | 0.42e9, 2.01e9 | 0.43e9 |
| 2448×1632 | 16 | dust r1 / r2 | 0.39e9, 2.11e9 / 2.62e9 | 0.12e9 | 0.41e9, 1.88e9 / 2.39e9 | 0.42e9 |
| 2448×1632 | 8 | reduce noise | 0.47e9, 3.27e9 | 0.34e9 | 0.43e9, 2.03e9 | 0.35e9 |
| 2448×1632 | 8 | dust r1 / r2 | 0.46e9, 3.13e9 / 3.65e9 | 0.34e9 | 0.42e9, 1.89e9 / 2.41e9 | 0.34e9 |
| 3462×2308 | 16 | reduce noise | 0.77e9, 4.49e9 | 0.24e9 | 0.83e9, 4.02e9 | 0.83e9 |
| 3462×2308 | 16 | dust r1 / r2 | 0.76e9, 4.22e9 / 5.24e9 | 0.24e9 | 0.81e9, 3.76e9 / 4.78e9 | 0.82e9 |
| 3462×2308 | 8 | reduce noise | 0.91e9, 6.53e9 | 0.67e9 | 0.83e9, 4.05e9 | 0.68e9 |
| 3462×2308 | 8 | dust r1 / r2 | 0.90e9, 6.27e9 / 7.29e9 | 0.67e9 | 0.82e9, 3.79e9 / 4.81e9 | 0.67e9 |
| 4242×2828 | 16 | reduce noise | 0.95e9, 6.74e9 | 0.35e9 | 1.04e9, 6.04e9 | 1.04e9 |
| 4242×2828 | 16 | dust r1 / r2 | 0.64e9, 6.39e9 / 7.92e9 | 0.35e9 | 0.78e9, 5.69e9 / 7.22e9 | 0.78e9 |
| 4242×2828 | 8 | reduce noise | 1.16e9, 9.81e9 | 1.01e9 | 1.05e9, 6.08e9 | 0.82e9 |
| 4242×2828 | 8 | dust r1 / r2 | 0.76e9, 9.46e9 / 10.99e9 | 1.01e9 | 0.79e9, 5.74e9 / 7.27e9 | 0.44e9 |

Lanczos3 stand-in plans equal the bicubic ones in bytes (work rises; every cell within 2 GiB). Rescaled
(`reduce_noise`, a committed ×0.5, then ×0.8 / ×1.6): the true preview refuses in every cell (3462×2308 and
4242×2828, both samplers, both depths); the stand-in plans 0.06e9 / 0.12e9 (16-bit 3462), 0.08e9 / 0.18e9
(16-bit 4242), 0.38e9 / 0.47e9 (8-bit 3462) and 0.57e9 / 0.70e9 (8-bit 4242). Fidelity (j), 600×400:
×0.9 stand-in vs true preview mean |Δ| 0 (max 0) for both filters, samplers and depths; after a committed
×0.5, ×0.8 mean 0.25–0.27 u16 codes (max 7) bicubic and 11.0 (max 3,931) Lanczos3 at 16 bits, 10.0–10.3
(max 514) and 20.8–21.1 (max 3,855) at 8 bits — all ≤ 257; ×1.6 (never substituted) stand-in vs true preview
mean 24.1 / 34.9 (16-bit bicubic / Lanczos3) and 33.7 / 44.4 (8-bit), max 16,962–17,990, the true preview
equal to the commit (mean 0): the record behind the enlargement guard.

**Probe** (`scripts/jobs-probe`, "filtered placed cmd-t preview" / "rescale"; ms from the scale change to
landing; first = 90 % Catmull-Rom, cold, second = the Lanczos3 switch):

| size, filter | 16-bit | 8-bit |
|---|---|---|
| 1224×816 `reduce_noise` (true preview) | 205 / 320 ms | 270 / 342 ms |
| 2448×1632 dust r2 (stand-in) | 435 / 863 ms | 360 / 661 ms |
| 3462×2308 `reduce_noise` (stand-in) | 729 / 1,598 ms | 589 / 1,199 ms |
| 4242×2828 `reduce_noise` (stand-in, INFO) | 1,143 / 2,431 ms | 903 / 1,767 ms |

a4a5859 (the true preview, before the measured charges) took 1.328 s cold / 0.552 s warm at 8 MP 8-bit
bicubic and refused 12 MP. Rescale (3462×2308, after a committed 50 %): 160 % and 160 % × 50 % land "Preview
unavailable: source transform exceeds the bounded sampling work limit" in 0–2 ms at both depths and
samplers; 80 % lands the stand-in in 344 / 535 ms (16-bit Catmull-Rom / Lanczos3) and 272 / 399 ms (8-bit),
then commits with the layer retained and filtered, no alert.

**Drive** (own background instance, port 4979, `MallocLargeCache=0`; no `[AutomationAlerts]` line):

| row | Phase-0 (23d7775) 16 / 8 | final 16 / 8 |
|---|---|---|
| A1 | ok 2.0 s / ok 1.7 s | ok 2.0 s / ok 1.8 s |
| A2 stack | host-refused / ok 7.8 s | **ok 8.1 s** / ok 7.9 s |
| A3 ⌘T, undo, redo | ok 5.6 s / "2.02 GB on its own" | ok 5.6 s / **ok 4.3 s** |
| A3-0 | ok 4.5 s / host-refused | ok 4.5 s / **ok 3.3 s** |
| A4-W | ok 2.8 s / ok 2.4 s | ok 2.7 s / ok 2.6 s |
| A4-P | host-refused / ok 2.4 s | host-refused (ceiling) / ok 2.5 s |
| B1 Replace | work cap / work cap | **ok 2.4 s / ok 1.7 s** |
| B3 Convert | work cap / work cap | **ok 4.7 s / ok 2.1 s** |
| B4 Assign | by name; `rasterize_sources` ok | same |
| B2 / B2-0 RAW redevelop | host-refused / host-refused | refused at the 2 GiB reference by the measured RAW decode charge (4.31 GB); admitted at 8 GiB: B2 ok 1.9 s / 1.7 s, B2-0 ok 0.9 s / 0.8 s |
| D1 Replace, D2 Convert, D3 RAW at 8 MP `reduce_noise` | work cap ×3 / work cap ×3 | **ok 1.1, 1.9, 1.0 s / ok 0.9, 1.0, 0.9 s** |
| SEQ: σ12, stack, ⌘T | stack host-refused, ⌘T ok / stack ok, ⌘T "2.61 GB" | stack host-refused, ⌘T ok / stack ok, ⌘T host-refused |
| 45 MP 8-bit ⌘T | — / "3.81 GB" | — / "2.13 GB on its own" (ceiling) |

Phase-0's 8-bit B2, B2-0 and D3 rows ran on the 16-bit RAW document (the drive's first `convert_precision`
call lacked `structure: preserve` and was refused), so their Phase-0 8-bit column repeats the 16-bit
behaviour; the final drive converted them. The sequence rows are information: SEQ-16's stack is refused with unique retained 1,253,549,973 B before
the call (581,537,357 history, 195,211,931 of it incremental; 384,000,000 cache) and 869,549,973 after,
phys_footprint 858 MB; SEQ-8's ⌘T after the stack plans 288,031,195 + 1,893,455,104 B (was 2.61 GB) and is
refused with 768,050,791 retained (480,034,863 history, 192,000,048 cache), footprint 663 MB. B2-0 is
refused at the 2 GiB reference with 529,143,190 (16-bit) / 433,145,752 (8-bit) retained: the measured RAW
decode charge `RawSourceWork.scratch` (173 B/px + 9 MiB + 3× the file = 4,305,440,196 B at 24 MP)
is above the limit on its own, filter or not; at
8 GiB it is admitted beside the same retained bytes (the RAW decode charge's R16/R8 and RF16/RF8 drive rows).

**Ceilings** (README): A4-P-16 (host admission, above), B2 at 24 MP (refused at the 2 GiB reference by the
measured RAW decode charge, 4.31 GB; admitted at 8 GiB), the 45 MP 8-bit
⌘T over a Background (byte plan 2.13 GiB alone), an enlarging or perspective ⌘T preview of a filtered placed
layer that the interactive cap refuses (stays "Preview unavailable"), and the sequence refusals (history).

## Open items (verified 2026-09-26)
- `DONE` — The 9A qualification drive is repaired: fixtures live in `~/devel/rasterize-fixtures/phase9a` (camera via `scripts/raw-probe/camera.py`, movie via the `write_h264_distinct` helper), and the host stage is ported to the 9B stack API and `LiveFilterSheetController` with new stack UI steps. `verify_phase9a.py --mode all --require-current` passed on a fresh `make app` over `824ea31` plus this repair: 119 host groups, 30 MCP groups. The NEF hang was environmental: it passed on Sep 11, hung on Sep 12 on the same commit, was cleared by the Sep 16 reboot, and ran 6 of 6 clean on `26bcd3c`; a `sample` tripwire (`nef-hang-sample.txt`) is in place.
- `TRACKED` — P7-12 gate half (advanced refinement brushing, colour decontamination; assisted session deferred) and therefore Phase 9 completion → carryover P7-12.
- `TRACKED` — Larger interactive filter/retouch workloads beyond the stated ceilings → P7-31 (§13A spatial neighborhoods), P7-33 (§13D).
- `TRACKED` — Live filters and retained sources under linear-light evaluation → P7-24 (11E phase 2); on float documents → plan §11F open list.
- `DONE` — 9D-1's two confirmed review findings: plate host admission (`RasterCore.retouchSource` admits scratch and passes a real `RasterJobControl`) and the 16-bit ghost latch (`EditorViewController+Heal.strokeSourceImage` uses the native `precisionStroke.sourcePlate`) — `bf706bb`.
- `DONE` — Filter-preset library whole-library strictness (a newer preset broke list/apply/delete): now per-entry `broken` rows on the shared `PreferenceLibrary` (`app/Sources/LiveFilterPresetLibrary.swift`, `bc3c0fc`).
- `DONE` — Typed destructive spatial filters (9F, P7-06); P7-16/P7-17 typed retouch (9D).
- `DONE` Part B + Part D phase 2 — the core plans admit live Gaussian σ1/σ2/σ4, Unsharp Mask, High Pass and Dust & Scratches r2 at 24 MP at both depths, and the ⌘T commit of a σ4 layer, under measured plans; the live Gaussian family runs `sampling_gaussian_fast` from σ4 up (section above). In the app, the 8-bit edit, re-edit and stack and the 16-bit first σ4 edit are admitted, and since Part E the 16-bit re-edit and ⌘T too; the 16-bit stack and the 8-bit ⌘T of a filtered layer still refuse.
- `DONE` Part C — Reduce Noise streams its guided-filter statistics through row rings (`sampling_box::BoxRows`), bit-identical, so live and selected Reduce Noise admit 24 MP in the core plans; Dust & Scratches and Reduce Noise carry measured work charges (`phase-9f-selected-tone-status.md`, "Reduce Noise statistics in row rings"). Part E is the next line.
- `DONE` Part E — a Gaussian-family last stage above the cache limit runs in row bands, bit-identical, so the core plans admit an 8-bit 45 MP σ4 live edit, its preview and its source `transform` commit (section above). In the app the 45 MP σ4 edit is admitted; the 45 MP 8-bit ⌘T is refused by the byte geometry plan and its `.rz` save, with or without the Background, by the 8-bit save estimate (fixed since by the core save plan, see the phase-4 entry on the 8-bit native save). Still open: banded prefix stages (multi-filter stacks at 45 MP), Rank / Denoise band arms, and 16-bit at 45 MP.
- `DONE` — Dust & Scratches as a banded last stage, the byte one-shot ⌘T plan without the never-read raster, one-shot filtered Replace and profile conversion, and the ⌘T preview stand-in (section above). Every row of that section is admitted in the app or a stated ceiling: A4-P-16 (host admission), B2 at 24 MP (refused at the 2 GiB reference by the measured RAW decode charge, 4.31 GB; admitted at 8 GiB), the 45 MP 8-bit ⌘T over a Background, and the enlarging or perspective ⌘T preview of a filtered placed layer. Open: a Denoise band arm, the enlarging/perspective preview, and the byte ⌘T plan's unselected-layer ×6 and canvas ×8 terms.
- `DROPPED` — Black-box proof that Normal reproduces 9A arithmetic via `blend_of → None`: accepted as unobservable; confirmed by code reading against `c725d88` (original, "residual gap").
- `DROPPED` — Making `sample: "all"` free on byte documents by reusing the held projection: review suggestion only; 9D chose admitted plates.
