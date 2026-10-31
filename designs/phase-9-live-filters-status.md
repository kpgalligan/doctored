# Phase 9 — editable photographic filters and retouching (ADR)

- **Status:** Accepted — open items remain ([#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11)) (9A–9H closed within scope; P7-12's other half shipped 2026-10-11 without the 9I quality gate)
- **Phase / dates:** 9A–9D recorded here, 9E–9I summarized with their own ADRs; 2026-09-11 → 2026-09-12; issue records added through October 2026
- **Originals:** [archive/phase-9-live-filters-status.md](../archive/phase-9-live-filters-status.md) — the phase's full history, to 2026-09-26, with review findings; [archive/2026-10/phase-9-live-filters-status.md](../archive/2026-10/phase-9-live-filters-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `c725d88` (9A), `33855da` (9B), `decada4` (9C), `01a5e51` (9D-1), `bf706bb` (9D), `694c39f` (9E), `2e7717d` (9F), `f457cdb` (9G), `93a45cf` (9H)
- **Removed 2026-10-10:** the live filter stack (9A–9C: the binding-owned stack, its caches, previews, presets and
  MCP tools). The sections below that recorded it are cut to pointers into the October original, and every open
  item about a live filter was removed with it. Gaussian Blur,
  Unsharp Mask, High Pass, Reduce Noise and Dust & Scratches remain as destructive filters on the same kernels
  (`core/src/filter_kernels.rs`). Retouching (9D–9I) is unchanged. An old `.rz` opens a live-filtered layer as the
  pixels it last composited. See [the removal ADR](convert-to-smart-object-status.md).

## Context
Phase 8 made layer content derive from retained originals ([phase-8-retained-source-status.md](phase-8-retained-source-status.md)).
Phase 9 added non-destructive photographic filters on that binding (since removed) and upgraded retouching so it
samples the right picture at native precision, never a display image. It verified P7-06–P7-11 and P7-13–P7-18;
P7-12's other half (edge brushing, colour decontamination) shipped 2026-10-11 without the cutout quality gate.

## Decisions — live filter stack (9A–9C)
Removed with the stack; the record is in [the October original](../archive/2026-10/phase-9-live-filters-status.md).
The kernel decisions that the destructive filters keep are documented at the kernels: the guided-filter Reduce
Noise in `core/src/sampling_denoise.rs`, the lower-median Dust & Scratches in `core/src/sampling_rank.rs`.

## Rejected alternatives
- Convolution-family or bilateral denoise — cost grows with radius; the guided filter's does not.

## Consequences
- The live stack's interactive and size ceilings went with it; the destructive filters' admission is recorded in
  [9F](phase-9f-selected-tone-status.md).

## Qualification
The live stack's qualification (9A–9C, including the 9C noise-cleanup photographic gate) is in the October
original; its probes and drives were removed with it. 9D's retouch qualification is below.

### Live filters at 24 MP, both depths
Removed with the stack (the October original). The σ4 Deriche switch it recorded holds for every Gaussian caller
and is documented in `core/src/sampling_gaussian.rs` and
[9F](phase-9f-selected-tone-status.md#a-radius-independent-legacy-gaussian-2026-10-03).

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
Removed with the stack (the October original, "Part E — banded last stage").

## Operations on a live-filtered layer
Removed with the stack (the October original). The byte one-shot ⌘T arm it amended still admits a RAW layer's
transform; its admission is documented in `core/src/doc_source_geometry.rs` (`# Admission: the one-shot set
transform`).

## Open items (verified 2026-10-30)
- `DONE` — P7-12's other half: edge brushing and colour decontamination shipped 2026-10-11 in Select and Mask; the quality gate ([gate v2](phase-9i-gate-v2-status.md)) is no longer a ship condition.
- `OPEN` [#9](https://github.com/kpgalligan/doctored/issues/9), [#11](https://github.com/kpgalligan/doctored/issues/11) — Larger interactive filter/retouch workloads beyond the stated ceilings: regional spatial neighbourhoods (P7-31) and the measured 24/48 MP corpus (P7-33).
- `DONE` — 9D-1's two confirmed review findings: plate host admission (`DoctoredCore.retouchSource` admits scratch and passes a real `RasterJobControl`) and the 16-bit ghost latch (`EditorViewController+Heal.strokeSourceImage` uses the native `precisionStroke.sourcePlate`) — `bf706bb`.
- `DONE` — Typed destructive spatial filters (9F, P7-06); P7-16/P7-17 typed retouch (9D).
- `DONE` — Reduce Noise streams its guided-filter statistics through row rings (`sampling_box::BoxRows`), bit-identical; Dust & Scratches and Reduce Noise carry measured work charges (`phase-9f-selected-tone-status.md`, "Reduce Noise statistics in row rings").
- `DROPPED` — Making `sample: "all"` free on byte documents by reusing the held projection: review suggestion only; 9D chose admitted plates.
- `DROPPED` (2026-10-30) — the 8-bit placed ⌘T at 100 MP (about 10.6 GB, over a 16 GB Mac's 8 GiB): only RAW layers remain placed, a RAW opens at 8 GiB only to about 90 MP, and the RAM-scaled limit admits it on larger Macs.
- `DROPPED` (2026-10-30) — working-memory estimate follow-ups, including the legacy 16-bit `blur` σ4 work margin: calibration of shipped admission figures with no user-facing failure.
