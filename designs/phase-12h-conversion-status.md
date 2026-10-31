# Structure-preserving precision conversion (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 12H (carryover P7-30, verified), runs 12H-1 and 12H-2, 2026-09-23. Closes Phase 12.
- **Original:** [archive/phase-12h-conversion-status.md](../archive/phase-12h-conversion-status.md) — full history, measurements, receipts
- **Key commits:** `754fa97` (12H-1), `a38f064` (12H-2)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Converting a document between 8-bit, 16-bit and 32-bit float used to flatten it. 12H adds an optional
`structure: "preserve"` conversion that carries every record it can, names what renders differently, what is discarded
and what blocks, and refuses rather than silently changing meaning. Flatten stays the default, byte-for-byte unchanged.
The one hard boundary was **no conversion into float** (11F, 12G-2's paste rule, the `convert_precision` catalog);
REVERSED by the user on 2026-10-12: 8→float and 16→float exist, preserve only (below the table).

## The table
Cells: **exact** = carried unchanged; **RD** = carried and listed in the plan's `rendering_differences`; **discarded** =
named in `discarded`; **blocks** = a `blocked` row, conversion refused with the document unchanged. Document-level rows
have id 0 and read "Preserve Layers cannot convert this document: <note>". The "→ float" directions (8→float,
16→float) were omitted as refused; that is reversed, and they are not columns here: their rule follows the table.

| Structure | 8→16 | 16→8 | float→16 | float→8 |
|---|---|---|---|---|
| Raster pixels | exact ×257 (`promote_samples`) | per record via `quantize_with_context` (RGB nearest/ordered, alpha nearest) | `tone_mapped_u16` per record · RD (per-record tone map ≠ flattened composite above SDR white / non-clip map) | tone map then quantize · same RD |
| Offset, name, opacity, visibility, clipping, locks, links, labels, comps, guides, ruler origin, layer ids, PPI, ICC, capture metadata | exact (ids set directly, never re-minted) | exact | exact | exact |
| Blend mode | exact; a mode `precision_layer_blend::validate` refuses **blocks** | exact | exact | exact |
| Isolated group | exact · RD (f32 instead of byte rounding) | exact · RD (byte rounding) | exact | exact · RD |
| Pass Through group | exact; isolated-through-style carried as an isolated group · RD | exact | exact | exact |
| Group depth | never blocks (byte 10 < typed 16) | deeper than 10 **blocks**, as the FIRST rows | never blocks | **blocks** past 10 |
| Record count | > `precision_render::MAX_RECORDS` (1024) **blocks** as the first row | n/a | n/a | n/a |
| Adjustment layer | carried iff `PrecisionAdjustment::parse` accepts the byte meta under `evaluation: precision_v1` (Exposure/White Balance pinned to `legacy_srgb`) · RD; refusal **blocks** | typed description → byte meta on a 1×1 plane (canvas-sized transparent plane when MASKED) · RD; `from_meta` refusal **blocks** (unreachable backstop) | re-created · RD (applies after per-record tone map) | as 16→8 + tone-map RD |
| Text / shape description | exact; re-edit rasterizes at 16 | exact | exact | exact |
| Other raster `meta` (an unknown kind) | **blocks** | n/a | n/a | n/a |
| RAW source | `RetainedSource16`, `rendered` = the ×257 raster · RD; stale binding not carried · RD | `RetainedSource8`, `rendered` = the landed pixels · RD; stale as 8→16 | **blocks** when the master is a 32-bit float RAW development ("…redevelop it as a 16-bit SDR master or rasterize it first", `FLOAT_RAW_NARROW_REFUSAL`); else carried (`rendered` = the tone-mapped leaf; re-renders at 16 bits on the next edit) | **blocks** as float→16; else carried (`rendered` = the landed pixels; re-renders at 8 bits on the next edit) |
| Warp binding | `WarpBinding16`, ×257 originals · RD; stale as sources | `WarpBinding8`, nearest originals, every guard input tied to the landed layer · RD | n/a | n/a |
| Layer mask | exact ×257, properties verbatim · RD only if feather > 0 or density < 1 | nearest | exact | nearest |
| Saved channel | exact ×257, overlay carried, id re-minted | nearest, id re-minted | exact | nearest |
| Layer style (fill, effects, knockout, blend interior) | exact via `with_layer_style` · RD; oversize padded plane **blocks** | exact (`reported_style` merge → one byte style) | exact | exact (`reported_style` merge, as 16→8) |
| Style on an adjustment layer | exact (`precision_style_adjustment`) | exact (`reported_style` merge) | exact | exact (`reported_style` merge) |
| Blend If | exact · RD (16-bit ramp) | exact | exact | exact (in the `reported_style` merge) |
| Extended range | n/a | n/a | **discarded** (`extended_range`) | same |
| Evaluation policy | carried, tables rebuilt; `linear_light` with a typed refusal **blocks** | carried; byte refusal is a document row | `linear_light` | `linear_light` |
| Global light (non-default) | exact (`global_light`) | exact | exact | exact |
| Group open state | exact (`group_open_state`) | exact | exact | exact |

Vector masks ([v1-paths.md](v1-paths.md)): on the structure-preserving 8→16 and 16→8 conversions a
vector mask with no raster mask beside it is set again from its shape on the converted document; one with a raster mask
crosses as the plain mask it baked. The path list is carried with its ids by every conversion, flatten included.

Into float (`../core/src/precision_conversion_float.rs`, preserve only): each u16 code (a byte first ×257) is
decoded through the document profile's TRC to linear light, alpha `c/65535`, by the layers paste's own promotion
under Preserve; masks and channels stay u16, every record keeps its id, the result is float under `clip`; a style
and Blend If ride, with the note "style and Blend If composite in linear light". Blocks: a
blend mode, effect mode or adjustment float refuses (on the record as it is), an effective warp, a profile with no
matrix/TRC model, and a result too large to save and reopen at 32 bits. The 8→float plan drops the 8→16 Blend If,
source and warp notes for its float rows. Narrowing back returns every code.

## Decisions
- **Flatten is untouched.** `precision_conversion.rs`, `dr_doc_promote_flattened*`, `PrecisionModeConversion.losses` and
  the step `{mode, flatten:true, dither}` are byte-for-byte unchanged.
- **Schema.** `convert_precision` requires only `mode`; the handler enforces: `structure` absent → `flatten:true` required;
  `preserve` + `flatten:true` refused ("flatten=true contradicts structure=preserve").
- **Plan before op.** 8→16 (`core/src/precision_conversion_structure.rs`) builds every record over 1×1 placeholders, then
  runs `rebuild` → `build` → `with_evaluation` → `native_fits_geometry` with real extents, so constructor, policy and
  capacity refusals become plan rows; the op refuses with the first row before promoting any plane. Narrowings
  (`core/src/precision_conversion_narrow.rs`) walk `precision_structure_ops::flatten` once with one per-leaf mapping per
  direction and no new pixel math, then check native capacity (8-bit: `native_layers_fit_dimensions`,
  `native_masks_fit_dimensions`, `channels_fit_at`; 16-bit: `native_fits_geometry`). `narrow_structure_to_u8` also
  runs `native_save_preflight` on the result as a backstop.
- **Admission (8→16)** = byte inventory + output (Σ raster px×8 + Σ mask/channel px×2) + Σ `prepare_scratch_bytes` +
  4·bookkeeping + 4 MiB for linear-light tables.
- **Adjustment envelope.** Byte meta gains `evaluation: precision_v1`; Exposure/White Balance also gain
  `params.evaluation: legacy_srgb` when absent (the typed default `document_v2` would silently change meaning). Unknown
  keys block.
- **Carried bindings compare POINTERS.** A carried source/warp's `rendered_*` planes are the very allocations the landed
  record holds and its `profile` the target document's `Arc`; otherwise `detach_invalid_sources` / `clean_typed_layers`
  silently drop it on the next mutation. `rendered_pixels`/`rendered_mask` are `Some` exactly when the source's were.
- **Dry run records nothing** (`dry_run: true` skipped by `AgentServer.swift`'s recording guard). **One undo step**
  ("Convert to N-bit (Preserve Layers)"), `intentionalDepthConversion: true`, active row re-selected. Recorded step:
  `{mode, structure:"preserve"}` plus `dither` for rgba8.
- **Host.** Preserve radio at every depth; the sheet plans exactly as `PrecisionModeConversion.init` routes
  (`promoteStructurePlan` for byte sources, `narrowStructurePlan(targetDepth:)` for typed).

## Tolerances (fixed before measuring; used by `../scripts/verify_phase12h_conversion.py`)
- Widening samples bit-exact (== byte ×257) for rasters, masks, channels.
- Typed render vs f64 straight source-over oracle: ≤ 1 code of 65535. Preview delta: max ≤ 771, mean ≤ 128.5 (of 65535).
  Drive 8-bit render delta: max ≤ 3, mean ≤ 0.5.
- 16→8 dither none bit-exact `(v+128)/257`; ordered RGB within 1, alpha exact. float→16 equals `tone_mapped_u16`; for
  sRGB + clip within 1 code of `round(65535·srgb_encode(clamp(v,0,1)))` (float→8: of `round(255·…)`). Preview bounds as
  above in all three narrowings; the HDR fixture's deltas are recorded, never bounded.

## Rejected alternatives
- Conversion into float — standing owner-level refusal (11F, P7-25); REVERSED by the user on 2026-10-12.
- Carrying a stale source/warp binding — a save would drop it too; disclosed instead.
- A test for a linear-light byte document holding a typed-only-refused operator — unreachable: the byte refusal set is a
  superset of the typed one.

## Consequences
- A new record kind or layer field must get a row in this table for every direction, and a plan row (RD, discarded or
  blocks) rather than a silent change.
- Any new byte/typed binding must satisfy the pointer-identity guards or it will detach after conversion.

## Open items (verified 2026-09-26)
- `DONE` — Group open state (9790eb5) and the global light are typed
  `PrecisionDocument` fields carried both ways (`../core/src/precision_global_light.rs`, `PRESERVED` `global_light`).
  Forward compatibility: a typed `.doctored` file holding a non-default light writes word 58, which an older build refuses with
  its generic high-depth sentence (house policy, as for 55/57).
- `DONE` — The oversize style-plane refusal (shared `plane_pixels` gate) is tested past the limit,
  at the limit and at the capped pad, with no 100 MP of pixels (`core/tests/precision_conversion_structure.rs`,
  `an_oversize_style_plane_blocks_by_name_and_the_limit_itself_carries`).
- `DONE` — 16→8, float→16, float→8 preserve; source/warp carriage 8→16 and back; recovery round trip (`a38f064`,
  `core/tests/precision_conversion_narrow.rs`).
- `DROPPED` — Carrying stale bindings (not planned, above); the "no 8-bit evaluation" adjustment block (unreachable
  backstop, tested per op); the 8-bit save-capacity backstop is recorded behaviour, not work.
