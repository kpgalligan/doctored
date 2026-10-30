# Typed clipping chains — native v32 / `LCLP` (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10D clipping slice, 2026-09-14 (GIMP receipt 2026-09-16)
- **Original:** [archive/phase-10d-clipping-status.md](../archive/phase-10d-clipping-status.md) — full history, qualification detail, receipts
- **Key commits:** `f65a0d2` (slice), `502cc88` (GIMP R5)

## Context
Byte documents evaluate clipping masks; the 16-bit typed model had no `clipped` flag, so clipping
was dropped on precision documents and in 16-bit PSD import. This slice adds the flag to every typed record kind
and renders clip units with exactly the byte rule, so clipping survives typed edits, native saves,
snapshots/recovery, the shared UI/MCP/Actions path and 16-bit PSD in both directions.

## Decisions
- **Clip units are derived per level, bottom-first, from `clipped` alone:** an unclipped entry is a base, the
  consecutive clipped run above it its members; a clipped entry with no unclipped entry below it in its level is
  baseless and composites as unclipped.
- **Byte rule verbatim** (`doc.rs::composite_clip_group_into`, `doc_group.rs::composite_level_into`): the base
  renders into a transparent buffer at full opacity in Normal (its own mask applies), its alpha footprint is
  recorded, each contributing member composites with its own opacity/mode/mask and the buffer alpha is forced back
  to the footprint, then the buffer composites once with the base's mode and opacity. No new pixel math
  (`zero_plane`, `render_layers`, `checked_alpha`, `composite_source_into`, `blend_kind`). Later slices refine the
  footprint: a Blend If base clamps members to its UNWEIGHTED footprint ([Blend If](phase-10d-blend-if-status.md));
  a styled base clamps to the byte-plane shape ([layer styles](phase-10d-layer-styles-status.md)).
- **Superseded by CL2 ([#578](https://github.com/kpgalligan/doctored-dev/issues/578), Photoshop take1 and take2):** members
  no longer composite absolutely and get forced back; `clip_unit.rs` (Open / Settle / Close, every byte, typed, float,
  regional and CMYK member loop) composites them onto the base with its alpha frozen, relative to the same footprint.
  An opaque base renders bit for bit as before.
- **Special bases:** an adjustment base has an empty footprint (members contribute nothing, as in byte documents); a
  Pass Through base with a visible member, and a Pass Through member that has a base, convert to isolated Normal
  groups (the byte `is_isolated` rule); a base with no contributing member takes the plain path unchanged.
- **Two predicates kept apart:** CONTRIBUTION is `visible && !cut.excludes(index)`; ISOLATION of a Pass Through base
  reads the document's own `visible` flags, never a cut. Typed retouch/backdrop cuts are `style_composite::Cut`s over
  `render_cut_with_context` and no longer rewrite `visible`.
- **Plan charge:** `PrecisionLayer::with_clipped_members` sets `group_depth = max(base, members) + 1` and refuses at
  `MAX_GROUP_DEPTH` (records bound `> MAX_RECORDS`, per `fa336cf`); plus `count·4` bytes of recorded alpha per live
  clip level and three canvas passes of work.
- **Edit/FFI:** `{op:"layer_clipped",layer,clipped}` structure edit (no version, exact no-op → NULL); capability
  family `layer_clipped` on raster, group and adjustment. The existing Swift `withLayerClipped` / `layerClipped`
  facade branches to it, so Create/Release Clipping Mask, `set_layer_clipped`, Actions, the Layers badge and Undo
  need no depth code.
- **Native v32:** outermost conditional wrapper and an `LCLP` schema 1 table (u32 count, strictly ascending
  postorder indices) after every prior tail including MOUT (`../core/src/precision_clip_native.rs`). 29/31 never wrap
  32. Empty/unordered/duplicate/out-of-range tables, wrong schema, trailing bytes, `LCLP` under a non-32 outer and 32
  inside 29 refuse before any PNG decode. Releasing the last flag restores the prior writer's exact bytes.
- **16-bit PSD:** import attaches the clipping byte to rasters and folders (the clipping loss sentences are gone);
  export writes `clipped` on raster and folder records; closers and the flattened record stay 0.
- **Retouch:** new-layer output under a clipped Pass Through ancestor refuses ("…would apply its opacity, mask or
  clip twice; use in-place output"); a new retouch layer inherits its target's flag, and one above a clip base
  re-bases the members onto the repair (byte `retouch_layer_above`).

## Consequences
- The byte clip-group rule is the spec at both depths; qualification is `../core/tests/precision_clipping.rs`
  (f64 reference within 0.65 native codes, exact bit-identity cases, native framing, PSD round trip), plus
  `psd_depth16.rs` / `psd_constructs.rs` expectations.
- GIMP 3.2.6 wraps each clipping run in a synthesized folder with `clip-to-backdrop` members and matches the stored
  composite at 0 codes (receipt R5, `../scripts/verify_phase10_receipts.py --receipts R5`).

## Open items (verified 2026-09-26)
- `DONE` — GIMP reopen receipt R5 (`502cc88`); independent Python v32 decoder `read_clipping` in `../scripts/live-filter-probe/native.py` (checked by `verify_phase10d_blend_decoder.py`).
- `DONE` — Blend If (`7c130e2`), layer styles (`b8aa5b5`), adjustment blending (`ea4d9d1`), 10C-2 slice 3 (`fc70079`), knockout (P7-37 verified).
- `DONE` — typed subset grouping/ungrouping with `cleared_clip` reporting (`../core/src/precision_structure_ops.rs` `group_layers` / `GroupReport.cleared_clip`; P7-26 verified in 12E-1).
