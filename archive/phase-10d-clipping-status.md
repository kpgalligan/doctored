# Phase 10D: typed clipping chains

The [HSL slice](phase-10d-hsl-modes-status.md) has since added Hue, Saturation,
Color and Luminosity with conditional native v33/schema5 inside the v32 wrapper
described below (`32 → 29 → 33 → body/tails → LBLD5 → MOUT → LCLP`), and the
[threshold slice](phase-10d-threshold-modes-status.md) has added Hard Mix,
Darker/Lighter Color and Dissolve with v34/schema6 in the same position
(`32 → 29 → 34 → body/tails → LBLD6 → MOUT → LCLP`). The
[Blend If slice](phase-10d-blend-if-status.md) has since added typed Blend If
with v35/`LBIF` outermost (`35 → 32 → 29 → 34 → body/tails → LBLD6 → MOUT →
LCLP → LBIF`); a clip base carrying Blend If keeps its UNWEIGHTED footprint
and clamps its members to it, so they stay visible where the ramp hides the
base.

This bounded slice gives the 16-bit document model a persisted `clipped` flag on
every record kind and renders clip units in the typed f32 compositor with
exactly the byte compositor's rule (`doc.rs::composite_clip_group_into` and
`doc_group.rs::composite_level_into`). Clipping therefore survives every typed
edit, native saves, snapshots/recovery, the shared UI/MCP/Actions path and
16-bit PSD import/export. Blend If, layer styles/effects, knockout, Hard Mix and
the remaining modes stay outside; P7-20 stays open for Blend If and styles.

## Meaning, host routing and persistence

A clip unit is derived per level, bottom-first, from `clipped` ALONE: an
unclipped entry is a base and the consecutive clipped run above it are its
members; a clipped entry with no unclipped entry below it in its level is
baseless and composites as if unclipped. The base renders into a transparent
buffer at full opacity in Normal (its own mask applies there), its alpha
footprint is recorded, each contributing member composites through the ordinary
kernel with its own opacity, mode and mask and the buffer's alpha is forced back
to the footprint, and the finished buffer composites once with the base's mode
and opacity. No new pixel math: the kernel is `zero_plane`, `render_layers`,
`checked_alpha`, `composite_source_into` and `blend_kind`. An adjustment base
has an empty footprint, so its members contribute nothing, as in byte
documents. A Pass Through base with a visible member, and a Pass Through member
that has a base, convert as isolated Normal groups (the byte `is_isolated`
rule); a base with no contributing member takes the plain path unchanged.

Two predicates are kept apart, as the byte cut requires: CONTRIBUTION is
`visible && !cut.excludes(index)`; ISOLATION of a Pass Through base reads the
document's own `visible` flags, never a cut. The typed retouch and backdrop
cuts are `style_composite::Cut`s over `render_cut_with_context` (they no longer
rewrite `visible`), so a sampling plate never moves pixels below its target.

The plan charges each clip unit one full plane above whatever its base or a
member allocates (`with_clipped_members` sets `group_depth = max(base, members)
+ 1` and refuses at `MAX_GROUP_DEPTH` exactly as `isolated_normal` does), plus
`count * 4` bytes of recorded alpha per live clip level and three canvas passes
of work.

Host routing: the existing `withLayerClipped` / `layerClipped` facade branches
to the core's `{op:"layer_clipped",layer,clipped}` structure edit (no version, no
samples; an exact no-op is NULL) and the parsed `clipped` layer metadata, so
Layer > Create/Release Clipping Mask, `set_layer_clipped`, the recorded Action,
the Layers panel badge and Undo need no depth-specific code. The capability
family `layer_clipped` admits raster, group and adjustment targets.

Persistence: a clipping member adds the outermost conditional RZDC32 wrapper and
an `LCLP` schema1 table (u32 count, strictly ascending postorder indices) after
every prior tail, including MOUT: `32 → 29 → 31 → prior body/tails → LBLD →
MOUT → LCLP`. Neither 29 nor 31 ever wraps 32. Releasing the last flag restores
the prior writer's exact bytes. Schemas 1–4 and MOUT are untouched; the byte
reader already refuses 32.

PSD at 16 bits: the importer attaches the record's clipping byte to rasters and
folders (both clipping sentences are gone from the typed loss list; the shared
limitation now reads "both depths evaluate clipping"), and the exporter writes
`clipped` on raster and folder records; closers and the flattened record stay 0.

## Refusals

- `layer_clipped` on an out-of-range layer or with an unknown field refuses in
  plan and edit; no document is published.
- A clip unit whose `max(base, members)` depth reaches `MAX_GROUP_DEPTH`
  refuses with the group-depth message (render plan and render).
- Native: an empty `LCLP` table, an unordered/duplicate/out-of-range index, a
  wrong schema, trailing bytes, `LCLP` under a non-32 outer, and 32 nested
  inside 29 refuse before any PNG decodes.
- New-layer retouch output under a clipped Pass Through ancestor refuses with
  "new retouch layer inside a gated group would apply its opacity, mask or clip
  twice; use in-place output"; in-place output succeeds. A new retouch layer
  inherits its target's flag.

## Qualification

`core/tests/precision_clipping.rs` (eleven tests) covers, against independent
f64 references: (a) a raster base with alpha 0/3/32768/65535 columns, a mask
outside 0 at density 0.75, opacity 0.625 and Screen, with a Normal member and a
masked Multiply member at opacity 0.5 — every RGB within 0.65 native codes of the
unrounded clip-group equation, alpha exactly the base's gated alpha; (b) exact
bit-identity for a baseless bottom entry (root and nested), a base whose only
member is hidden, and an invisible base; (c) Pass Through base/member isolation
equalities and the adjustment-base rule; (d) clip units inside isolated and
pass-through groups, a clipped GROUP member, and the depth boundary (a root
group base of depth `MAX_GROUP_DEPTH` refuses, depth `MAX_GROUP_DEPTH − 1`
renders with plan depth `MAX_GROUP_DEPTH`); (e) the FFI plan/edit, exact
no-op NULL, refusals, `rz_precision_doc_info` and Arc sharing; (f) standalone 32
decoded by an in-test `LCLP` reader, `32 → 29 → 31` composition, exact prior
bytes on release, and the refusal set above with a corrupted PNG proving the
framing error wins; (g) preservation through duplicate, rename, visibility,
opacity, blend, mask set/remove, crop, resize and retouch new-layer output, plus
the "clip twice" refusal; (h) a 16-bit PSD round trip with clipping bytes 1
read by the independent `psd16_fixture` reader and the same projection on
reopen; (i) the contribution-only cuts. `psd_depth16.rs` now expects one loss and
`layers[1].clipped == true`; `psd_constructs.rs` expects no clipping sentence,
the clipped folder, and the typed projection equal to the 8-bit hand-computed
picture. The byte-identical suites (`precision_layer_blending`,
`precision_remaining_separable`, `precision_four_blending`,
`precision_blend_mask_wrappers`, `precision_groups`, `precision_prototype`,
`precision_document`, `psd_export_corpus`, `psd_export`) pass unchanged.

### Receipts as run (2026-09-14)

- `cargo fmt --check && cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --release --test precision_clipping`: 11 passed, 0 failed.
- The eleven byte-identical/PSD suites named above: all pass unchanged.
- `make test`: 161 test binaries report `ok`, none with a failure, exit 0.
- `make typecheck`: 0 errors (24 pre-existing deprecation warnings, the same
  count as the untouched baseline). `make app`: BUILD SUCCEEDED.
- MCP drive of the built app (port 4917, dummy key): `open_document` a 16-bit
  `.rz` with two layers → `set_layer_clipped` → `get_document` shows `clipped`
  → `render` shows the upper layer confined to the base (right half alpha 0)
  → `undo`/`redo` → `save_copy` `.rz` (header `RZDC` 32) and reopen with the
  flag → `export_psd` `bit_depth` 16 → `inspect_psd` (rgba16, no clipping
  loss) + `open_document psd_mode: layers` → `get_document` shows the flag;
  `set_layer_clipped` on an 8-bit document unchanged. No `isError`.

## Open

- GIMP reopen receipts for a 16-bit clipped export are TAKEN 2026-09-16
  (`~/devel/rasterize-fixtures/phase10-receipts/R5/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`; re-runnable as
  `scripts/verify_phase10_receipts.py --receipts R5`, which authors the
  document over MCP and hands it to `scripts/psd-exchange-probe/run_reopen.py`
  through the shared `verify_phase10b_external.gate_structure`). GIMP 3.2.6
  wrapped the file's one clipping run in a single synthesized folder with
  members `[Clip base, Clipped]` and `clip-to-backdrop` on the second only, and
  BOTH its merged preview and its layered projection matched the stored
  composite at 0 codes (threshold 514).
- The Python independent v32 decoder (`read_clipping` in
  `scripts/live-filter-probe/native.py`, checked by
  `verify_phase10d_blend_decoder.py`) has since shipped with the Blend If slice.
- Blend If has since shipped ([Blend If slice](phase-10d-blend-if-status.md))
  and layer styles too ([layer-styles slice](phase-10d-layer-styles-status.md):
  a styled clip base clamps every member to the byte-plane shape). Adjustment-layer blending has since shipped ([adjustment-blending slice](phase-10d-adjustment-blending-status.md): the same twenty-six modes on a typed adjustment under the byte `composite_adjustment_into` rule, conditional v37/schema7), which verifies P7-19; knockout has no byte implementation to migrate and is tracked as P7-37 under 12E.
  10C-2 slice 3 stays open.
- Typed subset grouping/ungrouping and its `cleared_clip` reporting (12E). A new
  retouch layer above a target that is itself a clip base re-bases the members
  onto the repair, exactly as the byte `retouch_layer_above` does.
