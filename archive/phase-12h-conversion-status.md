# Phase 12H — structure-preserving precision conversion: status

Slice 12H (carryover [P7-30](phase-7-carryovers.md#assignment-register)) is
TWO runs. **12H-1** writes the whole per-structure table below for all six
directions and ships the one widening direction that exists, **8→16**.
**12H-2** ships the three narrowings (16→8, float→16, float→8), retained-source
and warp carriage on 8→16 and recovery qualification; its criteria are
recorded at the foot of this doc as they were written before the run. **12H is
Shipped, P7-30 is VERIFIED and Phase 12 is shipped.**

| Run | What it owns | State |
|---|---|---|
| **12H-1** | The table; the core 8→16 plan/op/preview (`core/src/precision_conversion_structure.rs`); the Convert Precision sheet; MCP `structure` / `dry_run`; the drive | **Implemented 2026-09-23** |
| **12H-2** | 16→8, float→16, float→8 preserve (`core/src/precision_conversion_narrow.rs`); retained/smart/RAW/live-filter sources and warp bindings 8→16 and back; recovery qualification; the three 12H-1 review nits | **Implemented 2026-09-23** |

## The table

Cells: **exact** = carried with no evaluation change; **exact · RD** = carried,
listed in the plan's `rendering_differences` with the note given; **discarded**
= named in the plan's `discarded` array; **blocks** = a `blocked` row naming the
record, and the conversion refuses with the document unchanged. A
document-level row (id 0 — capacity, the evaluation policy, a channel) reads
"Preserve Layers cannot convert this document: <note>" with no layer prefix.
The narrowing columns shipped in 12H-2. **There is no conversion INTO float**
— [11F rows 30/385](phase-11f-hdr-float-status.md) ("Deliberately never"),
12G-2's paste rule and the `convert_precision` catalog sentence are a
standing contract — so both "→ float" columns read *refused by name: no
promotion into float* for every structure and are omitted from the cells.

| Structure | 8→16 (12H-1) | 16→8 (12H-2) | float→16 (12H-2) | float→8 (12H-2) | 8→float / 16→float |
|---|---|---|---|---|---|
| Raster pixels | exact: every code ×257 (`promote_samples`) | quantized per record with the flattened path's quantizer (`quantize_with_context`: RGB nearest/ordered, alpha nearest) | `tone_mapped_u16` per record · RD on every float raster "tone-mapped per record before compositing; where a sample exceeds SDR white or the map is not clip the composite differs from the flattened conversion" | tone map, then quantize · the same RD | refused by name |
| Offset, name, opacity, visibility | exact | exact | exact | exact | refused by name |
| Blend mode | exact (every byte mode has a typed kernel); a mode `precision_layer_blend::validate` refuses **blocks** | exact | exact | exact | refused by name |
| Isolated group | exact · RD "isolated group composites in f32 instead of byte rounding" | exact · RD "isolated group composites with byte rounding instead of f32" | exact | exact · RD (byte rounding) | refused by name |
| Pass Through group | exact as `PassThrough`; when `doc_group::is_isolated` holds through a rendered style it is carried as an isolated `Group` holding that style · RD (noted) | exact | n/a (float has no Pass Through) | n/a | refused by name |
| Group depth | never blocks (byte `MAX_GROUP_DEPTH` 10 < typed 16) | a record nested deeper than 10 **blocks**, as the plan's FIRST rows ("nested N deep; an 8-bit document holds 10 levels") | never blocks (16 → 16) | **blocks** past 10, as 16→8 | refused by name |
| Document record count | > `precision_render::MAX_RECORDS` (1024) **blocks**, as the plan's FIRST row at index 1024 ("N layers exceed the 1024-record limit of a 16-bit document"); the byte document has no in-memory cap (`MAX_RZDC_LAYERS` applies only at save) | n/a | n/a | n/a | refused by name |
| Adjustment layer | carried iff `PrecisionAdjustment::parse` accepts the byte meta under the typed envelope (`evaluation: precision_v1`; Exposure/White Balance keep the byte default `legacy_srgb` explicitly) · RD "typed f32 math instead of the byte LUT"; a refusal **blocks** naming the op and the layer | the typed description becomes the byte meta (the byte parser ignores `evaluation`) on a 1×1 plane, or on a canvas-sized transparent one when MASKED (a byte adjustment's mask covers only its pixel extent) · RD "evaluates the byte LUT instead of typed f32 math"; a description `Adjustment::from_meta` refuses **blocks** — a BACKSTOP, unreachable through the typed parser, which runs the byte `from_op` itself | re-created at 16 bits · RD "adjustment applies after the per-record tone map" | as 16→8, plus the tone-map RD | refused by name |
| Text / shape description | exact (`artwork_document::validate`); a re-edit re-rasterizes at 16 bits | exact; re-edit at 8 bits | exact | exact | refused by name |
| Other `meta` (Live Photo, host media, unknown) | **blocks** | n/a (a typed record holds only a validated text/shape description) | n/a | n/a | refused by name |
| Retained / smart / RAW / live-filter source | carried: `RetainedSource16` whose `rendered` IS the record's ×257 raster (a fresh live-filter cache) · RD "source re-renders at 16 bits on its next edit; its cached rendering is widened ×257" (+ "live filters re-evaluate at 16 bits"); a STALE binding is not carried · RD "stale binding not carried; a save drops it too" | carried: `RetainedSource8` whose `rendered` IS the landed `Layer.pixels` and `rendered_profile` the document's · RD "source re-renders and quantizes its fallback at 8 bits on the next edit"; stale as 8→16 | n/a (float holds none; one would block) | n/a (as float→16) | refused by name |
| Warp binding | carried: `WarpBinding16` with ×257 originals, `rendered_*` the record's own raster/mask plane, `profile` the document's `Arc` · RD "warp re-edits at 16 bits from the widened original"; stale as sources | carried: `WarpBinding8` with nearest originals and EVERY `doc_warp` guard input tied to the landed layer (`rendered_pixels`/`rendered_mask` the `Layer.pixels`/`Layer.mask` Arcs, their offsets, the document profile `Arc`) · RD "warp's retained original quantized to 8 bits" | n/a (float has none; one would block) | n/a | refused by name |
| Layer mask | exact: plane ×257, `mask_properties` verbatim, `mask_enabled`; · RD only when feather > 0 or density < 1 | plane nearest, properties verbatim | exact (u16 plane verbatim) | nearest | refused by name |
| Vector mask | n/a — not a Rasterize record kind | n/a | n/a | n/a | n/a |
| Saved channel | exact: plane ×257, overlay colour/opacity/selected side; id re-minted (never persisted); provenance none | nearest, overlay side carried; id re-minted | exact | nearest | refused by name |
| Layer style (fill, effects, knockout, blend interior) | exact via `with_layer_style` · RD "typed style planes"; oversize padded plane **blocks** | exact: the `reported_style` merge of the two typed fields as ONE byte style | n/a (float refuses a style by name) | n/a | refused by name |
| Style on an adjustment layer | dropped, disclosed as RD (it renders nothing at 8 bits either) | n/a (a typed adjustment holds none) | n/a | n/a | refused by name |
| Blend If | exact (the typed layer field) · RD "16-bit ramp (10D)" | exact (into the byte style) | n/a (float refuses by name) | n/a | refused by name |
| Clipping, locks, links, labels | exact | exact | exact | exact | refused by name |
| Layer comps | exact (`LayerComp` list; entries bind by the carried ids) | exact | exact | exact | refused by name |
| Guides, ruler origin | exact | exact | exact | exact | refused by name |
| Layer ids | exact — set directly, never re-minted (`doc_identity`'s one counter) | exact | exact | exact | refused by name |
| PPI, ICC profile, capture metadata | exact | exact | exact | exact | refused by name |
| Extended range (float) | n/a | n/a | **discarded** (`extended_range`): every sample above SDR white or below zero is mapped by the tone map | same | refused by name |
| Evaluation policy | carried, tables rebuilt at 16 bits (`with_evaluation`); `linear_light` with a typed refusal **blocks** — never a silent policy change | carried, byte tables rebuilt; a byte refusal is a document row | `linear_light` | `linear_light` (byte) | refused by name |
| Global light (non-default) | **discarded** (`global_light`) + an RD row per styled layer: effects read the default light | n/a (the byte document takes the default, which is what the typed effects read) | n/a | n/a | refused by name |
| Group open state | **discarded** (`group_open_state`): the typed model has no field | every group lands OPEN (nothing to discard: the typed document had no state) | n/a | open, as 16→8 | refused by name |

## Decisions (12H-1)

- **Flatten stays the default and byte-for-byte.** `precision_conversion.rs`,
  its tests, `rz_doc_promote_flattened*`, `PrecisionModeConversion.losses` and
  the flatten record step `{mode, flatten:true, dither}` are unchanged. An
  absent `structure` still requires `flatten:true`.
- **One walk, two uses.** The plan builds every record over 1×1 placeholder
  planes, then runs `rebuild` → `PrecisionDocument::build` →
  `with_evaluation` → `native_fits_geometry` with the REAL raster/mask extents
  and channel count, so a constructor, policy or capacity refusal becomes a
  plan row. The op refuses with the plan's first row before any canvas-sized
  plane is promoted, then walks again with promoted planes.
- **Admission** = the byte document's inventory (retained) + output (Σ raster
  px×8 + Σ mask/channel px×2) + Σ `prepare_scratch_bytes` + 4·bookkeeping +
  4 MiB for the rebuilt linear-light tables when the policy is `linear_light`.
- **Adjustment envelope.** The byte meta gains `evaluation: precision_v1`;
  Exposure and White Balance also gain `params.evaluation: legacy_srgb` when
  absent, because the typed parser defaults them to `document_v2`, which would
  silently change their meaning. Any key the typed parser does not know blocks.
- **Schema.** `convert_precision` lists only `mode` as required; the handler
  is the sole enforcer of the flatten rule (`structure` absent/`flatten` →
  `flatten:true` required; `preserve` + `flatten:true` refused by name as
  "flatten=true contradicts structure=preserve"; `preserve` + `flatten:false`
  accepted).
- **Dry run records nothing.** `AgentServer.swift`'s one recording guard skips
  `convert_precision` with `dry_run == true`; `ActionCatalogFacts.nonRecordable`
  is per-tool and is not touched.
- **One Undo step.** `publish` keeps `intentionalDepthConversion: true`; the
  action is "Convert to 16-bit (Preserve Layers)" and the previously active
  row is re-selected (indices are identical by construction).

## Decisions (12H-2)

- **No conversion into float** (owner-level, recorded): 8→float and 16→float
  stay refused by name (11F, P7-25). The narrowings are the only new
  directions.
- **One walk, three mappings.** `precision_conversion_narrow` walks
  `precision_structure_ops::flatten` once, bottom-first, with ONE per-leaf
  mapping per direction and no new pixel math: `quantize_with_context`
  (16→8), `tone_mapped_u16` (float→16), the two in sequence (float→8); masks,
  channels and warp originals go through `precision_quantize::nearest`. The
  plan walks 1×1 SHAPES and assembles the shaped target, then runs its native
  capacity with the real extents (`native_layers_fit_dimensions` +
  `native_masks_fit_dimensions` + `channels_fit_at` at 8 bits,
  `native_fits_geometry` at 16), so a capacity or policy refusal is a document
  row first. The op refuses with the plan's first row before any plane is
  built, and the byte result passes `native_save_preflight` as a backstop.
- **Table correction.** The byte `doc_group::MAX_GROUP_DEPTH` is 10 and the
  typed one 16, so an 8-bit target BLOCKS a record nested deeper than 10 — as
  the plan's FIRST rows, before any other kind. 12H-1's "never blocks" cell
  was wrong for the narrowing columns.
- **Guards compare POINTERS.** A carried binding's `rendered_*` planes are the
  very allocations the landed record holds and its `profile` the assembled
  document's `Arc`, because `detach_invalid_sources` (byte) and
  `clean_typed_layers` (typed) silently drop a binding that fails them on the
  next mutation. `rendered_pixels` / `rendered_mask` are `Some` exactly when
  the source binding's were (a Mask-target warp has no rendered pixels — on a
  text layer or a group, `Some` would detach it). A STALE binding (its
  effective query answers `None`) is not carried and gets an RD row; a save
  would drop it too. At 8→16 a sourced record's SHAPE carries its real extent,
  because `build` validates the binding against its fallback.
- **Document rows** have id 0 and read "Preserve Layers cannot convert this
  document: <note>" in the core, `PrecisionModeConversion.refusal(plan:)` and
  the sheet (`document: <note>`) alike.
- **Adjustments at 8 bits.** The typed description becomes the byte meta
  verbatim — the byte parser ignores the `evaluation: precision_v1` envelope
  and reads `params.evaluation` `document_v2` / `legacy_srgb` itself. The "no
  8-bit evaluation" block is a BACKSTOP: the typed parser runs the byte
  `Adjustment::from_op`, so every admitted typed description is byte-readable
  (tested per op).
- **Linear-light byte document with a typed-refused operator — UNREACHABLE.**
  12H-1's nit asked for a test; the byte `refused_operator` refuses a superset
  of the typed one (retained source, style, the seven blend modes, a guided
  Shadows/Highlights), so no linear-light byte document can hold an operator
  only the typed walk refuses. Recorded here instead of tested.
- **Host.** The Preserve radio is enabled at every depth; the sheet's plan is
  routed exactly as `PrecisionModeConversion.init` routes it (byte source →
  `promoteStructurePlan`, typed → `narrowStructurePlan(targetDepth:)` for the
  current mode, so the float popup re-plans); the recorded step is
  `{mode, structure:"preserve"}` plus `dither` for rgba8; the action is
  "Convert to N-bit (Preserve Layers)"; an unreadable `mode` reads "Specify
  mode rgba8 or rgba16."

## Tolerances (fixed before measuring)

- Widening samples bit-exact (== byte ×257): rasters, masks, channels.
- Pointwise sub-fixture typed render vs an f64 straight source-over oracle:
  ≤ 1 code of 65535.
- Full-fixture preview delta: max ≤ 771 (3 of 255), mean ≤ 128.5 (0.5 of 255).
- Drive 8-bit render delta: max ≤ 3, mean ≤ 0.5.
- 12H-2, fixed before measuring: 16→8 samples with dither none bit-exact
  `(v+128)/257`; ordered RGB within 1 of that, alpha exact. float→16 samples
  equal `tone_mapped_u16` of the same leaf, and for sRGB + clip within 1 code
  of `round(65535·srgb_encode(clamp(v,0,1)))` (float→8: within 1 of
  `round(255·…)`). Preview delta on an in-range clip fixture max ≤ 771 /
  mean ≤ 128.5 in all three directions. Drive n2 8-bit render delta max ≤ 3 /
  mean ≤ 0.5. The HDR drive fixture's deltas are RECORDED, never bounded.

## Receipts

- Core: `core/tests/precision_conversion_structure.rs` — the full fixture's
  preview delta measured **max 363 / mean 49.13 codes of 65535** over 4800
  compared samples (isolated Screen group, Multiply masked layer with a
  2-px feather, clipped layer, Pass Through group with a Levels adjustment,
  text layer, drop-shadow + Blend If + Shallow knockout style).
- Drive: `scripts/verify_phase12h_conversion.py`, receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12h/drive/`. All eight gates
  PASS: g1 the dry-run plan (no blocked row; isolated group, feathered mask,
  Levels, style and Blend If rows) with the history cursor unchanged; g2 a
  dry run between `start_recording` and `stop_recording` records NO step; g3
  the listed schema requires only `mode`, `{structure:"preserve",
  flatten:true}` is refused by name and an absent `structure` without
  `flatten` still is, with the document unchanged; g4 preserve WITHOUT
  `flatten` converts with all nine rows equal in one vocabulary (id, name,
  kind, depth, parent, blend, opacity, visibility, clipping, mask and its
  properties, locks, link, label, style, text, adjustment op, raster offset),
  guides, ruler, PPI, channels and comps equal — preview delta **max 192 /
  mean 17.75 codes of 65535** over 6912 samples and the 8-bit render before
  vs the 16-bit render after **max 1 / mean 0.005 codes of 255**; g5 ONE undo
  gives a `.rz` save byte-identical to the pre-conversion save and redo
  re-applies; g6 save → reopen → resave byte-identical; g7 a placed image
  (retained source) blocks as "layer 9 "g7-place": an embedded or linked
  source original … is carried in 12H-2" with the document unchanged; g8 the
  recorded step is exactly `{mode: rgba16, structure: preserve}` and
  `run_action` converts a fresh fixture with the same rows.

### 12H-2 receipts

- Core: `core/tests/precision_conversion_narrow.rs` (14 tests) and the grown
  `core/tests/precision_conversion_structure.rs` (14). 16→8 of the 12H-1
  fixture carried to 16 bits returns every byte plane EXACTLY (×257 then
  nearest is the identity) and its 8-bit projection is bit-identical to the
  original's; on a non-×257 source nearest is bit-exact `(v+128)/257` and
  ordered within 1 with exact alpha. Preview deltas, all within the fixed
  bound: **16→8 max 363 / mean 49.13**, **float→16 max 409 / mean 11.50**,
  **float→8 max 514 / mean 112.58 (none), max 553 / mean 121.66 (ordered)**
  codes of 65535. float→16 and float→8 samples are within 1 code of the sRGB
  clip oracle; every float raster carries its RD row; `extended_range` is
  discarded; the u16 result is `linear_light`. A depth-11 nest blocks FIRST
  ("nested 11 deep; an 8-bit document holds 10 levels") and 10 levels
  convert. A retained source and three warp shapes (unmasked Pixels, Pixels
  on a masked layer, Mask target) come back effective, stay effective after
  `detach_invalid_sources` (`rz_doc_clone`), and their originals return
  exactly; a masked adjustment keeps its canvas extent (the byte compositor
  only covers a masked adjustment's pixel extent — the one correction the
  drive found, below). Byte `.rz` save → reopen → resave identical for the
  fixture, the warped and the placed documents; typed `.rz` for float→16.
  8→16: a retained source and a warp are carried (effective after
  conversion and after a `rebuild`), a stale binding is disclosed and not
  carried, invalid mask properties and a group carrying meta block by name,
  and an id-0 row reads with no layer prefix.
- Drive: `scripts/verify_phase12h_conversion.py`, receipts under
  `~/devel/rasterize-fixtures/phase12-receipts/12h2/drive/`. All sixteen
  gates PASS. g1–g8 as 12H-1 (g4 **max 192 / mean 17.75** of 65535, render
  **max 1 / mean 0.005** of 255), with g7 now CARRYING the placed source (same
  `source_id`/`revision_id`, RD "source re-renders at 16 bits…"). **n1** the
  rgba8 dry run (direction, no blocked row, the byte-rounding RD, history
  unchanged); **n2** 16→8 preserve — nine rows, chrome and comps equal,
  preview **max 192 / mean 17.75** of 65535, render **max 1 / mean 0.005** of
  255; **n3** one undo gives the pre-conversion 16-bit save byte-for-byte,
  redo re-applies; **n4** resave identical; **n5** float → rgba16 on the
  committed `float_11f/scene.tiff` layered over MCP (new layer, clipped
  duplicate, group, mask, lock, label, guide, comp) — four rows equal,
  `linear_light`, preview RECORDED **max 0 / mean 0**, undo/redo, resave
  identical; **n6** float → rgba8 ordered — rows equal, preview RECORDED
  **max 250 / mean 49.91**, the recorded step is exactly `{mode:"rgba8",
  structure:"preserve", dither:"ordered"}` and `run_action` replays it on a
  fresh copy; **c1** a placed source and a warp on `A` survive 8→16→8 with
  the same `source_id` and the same `get_warp` field and name their RD rows
  in both directions; **r1** `write_recovery` checkpoints of the n2 (8-bit)
  and n5 (16-bit) documents reopen and re-save **sha256-equal** to a save of
  the live document.
- The drive found ONE core defect before it passed: a MASKED byte adjustment
  layer's mask covers only its pixel extent, so the spec's 1×1 adjustment
  plane rendered the Levels over one pixel (n2 preview max 5481). A masked
  adjustment now lands with the canvas-sized transparent plane a byte host
  creates; an unmasked one keeps 1×1.

## Open, with owners

The three 12H-1 residual rows owned by 12H-2 — the narrowings, source/warp
carriage and recovery — CLOSED in 12H-2 (Receipts), and with them P7-30 and
Phase 12. What stays open:

| Item | Owner | Criterion |
|---|---|---|
| Global light and group open state at 16 bits | later (unscheduled) | Needs a typed field; until then both are `discarded` by name 8→16, and 16→8 lands the default light and every group OPEN. |
| Oversize style plane refusal is untested | later (unscheduled) | Constructing a > `MAX_PIXELS` padded plane needs a > 100 MP layer; the refusal is the shared `plane_pixels` gate. |
| Carrying a STALE source/warp binding | not planned | A stale binding is disclosed and not carried, exactly as a save drops it. |
| The "no 8-bit evaluation" adjustment block | not reachable | A backstop: the typed parser runs the byte `from_op`, so no admitted typed description can meet it (tested per op). |
| The 8-bit save-capacity backstop | recorded | `narrow_structure_to_u8` also runs `native_save_preflight` on the result, which can refuse a document (e.g. isolated-group scratch past `MAX_RZDC_GROUP_CANVAS_PIXELS`) the plan did not name as a row; the op still leaves the input untouched. |

---|---|---|
| 16→8, float→16, float→8 preserve | 12H-2 | Per-record quantization / tone map with dither, each direction's rows verified against the same fixed tolerances. |
| Retained/smart/RAW/live-filter sources and warp bindings 8→16 | 12H-2 | The typed binding re-validates via `effective_source`, a re-edit re-renders at 16 bits, disclosed. |
| Recovery qualification | 12H-2 | `write_recovery` / `recover_document` round trip digest-equal at the new depth. |
| Global light and group open state at 16 bits | 12H-2 or later | Needs a typed field; until then both are `discarded` by name. |
| Oversize style plane refusal is untested | 12H-2 | Constructing a > `MAX_PIXELS` padded plane needs a > 100 MP layer; the refusal is the shared `plane_pixels` gate. |
| P7-30 to verified; Phase 12 status to shipped | 12H-2 | After the three rows above. |

---

# 12H-2 — criteria (as written before the run)

- **Narrowing preserve.** 16→8, float→16 and float→8 keep every record the
  table lists: planes quantize per record with the flattened path's quantizer
  (RGB nearest/ordered, alpha nearest, never dithered), float rows run
  `tone_mapped_u16` per record, and each direction's rows are verified against
  the same fixed tolerances.
- **Sources and warps 8→16.** Retained/smart/RAW/live-filter sources and warp
  bindings are carried; the typed binding re-validates via `effective_source`
  and a re-edit re-renders at 16 bits, disclosed.
- **Recovery.** `write_recovery` / `recover_document` round trip digest-equal
  at the new depth.
- P7-30 to verified; the Phase 12 status line to shipped.
