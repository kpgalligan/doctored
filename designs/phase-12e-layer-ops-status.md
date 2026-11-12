# High-depth layer and document operations (ADR)

- **Status:** Accepted — open items remain ([#31](https://github.com/kpgalligan/doctored/issues/31): the Background mark and PSD layer comps in the colour modes; [#13](https://github.com/kpgalligan/doctored/issues/13): comp position and appearance)
- **Phase / dates:** 12E (12E-1 … 12E-3d), 2026-09-19 → 2026-09-20; closes P7-26, P7-27, P7-37
- **Original:** [archive/phase-12e-layer-ops-status.md](../archive/phase-12e-layer-ops-status.md): full history, measurement tables (M1–M7, P16/PD/PX), F1–F11 pre-registration, receipts, drive gate lists
- **Key commits:** `ab5c193` (12E-1), `9d49b2c` (1b), `a03fca9` (1c), `50b2d67` (2), `d8a0538` (2b), `e8af30c` (2c), `78cb0ba` (3), `1a69abb` (3b), `1063561` (3c), `e03026e` (3d)

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
Before 12E, 16-bit and float documents refused most layer-structure edits, layer ids did not persist, and
knockout was not implemented at any depth. 12E brings the byte document's structure, merge, lock, guide,
insertion, stamp and flatten contracts to the typed (`PrecisionDocument`) depths. It also designs Knockout and
"Blend Interior Effects as Group" and qualifies them against an f64 oracle and against Photoshop 26.10.0. The
byte op is always the specification: each typed twin restates it and adds no rules of its own.

## Decisions

### Native version chain (all of 12E)
- **Chains.** Byte `45 → [49] → 43 → 41 → 38 → 28 → body`; typed `46 → [47] → [48] → [50] → 44 → 42 → …`, tail
  tables `… → LCMP → LLNK → LGDE → LIDS`.
- **Invariants.** 45/46 is always the first word and `LIDS` always the last table, so new conditional words go
  *inside* 45/46. The Swift sniffer (`RasterDocument+PrecisionMetadata.swift`) and the test peel
  (`core/tests/common/identity.rs`) depend on this; a new outermost word must update both.
- **Every conditional word** is written only when its feature is present. Clearing the feature restores the
  prior writer's bytes, and reopen → resave is byte-identical. Older builds refuse the word by name, and the
  writer never emits what its own reader refuses.

### 12E-1: persisted layer ids and typed structure
- **`LIDS` (DRDC 45 byte / 46 typed, `identity_native.rs`) is always written.** Layout: u32 schema 1, a count,
  then u64 ids in bottom-first order (byte) or postorder (typed). *Why:* every layer has an id, so a conditional
  table would drop ids whenever unrelated state cleared. Every file therefore gets a new first word.
- **Ids are unique per document, not per file set.** The reader adopts a file's ids and raises the counter past
  them (`doc_identity::reserve_layer_ids_through`). Older files and PSD imports mint fresh ids. A zero or
  duplicate id, a count mismatch or a trailing word is refused before any PNG decodes.
- **Typed structure runs over a flat postorder list** (`precision_structure_ops.rs`): `flatten`, byte splice,
  `rebuild`, with allocations pointer-equal. It serves `move_layer_to`, `arrange_layer`, `group_layers`,
  `ungroup_layer` and `remove_layers` through `dr_precision_doc_edit[_plan]`; NULL means no-op. Move, delete and
  ungroup keep ids; grouping mints one for the container. *Why:* one splice instead of five recursive rewrites.
- **Float groups honour `pass_through`** as every other depth does since 11F slice 4 (until then they were isolated
  Normal groups and the core forced `pass_through=false`).
- **Host routing.** `RasterDocument` methods route typed on their first line; `DocumentPrecision` admits these
  families in the index-mapped branch (falling through to `["document"]` gives an unactionable refusal).

### 12E-1b: typed merge, locks/links, comps by id, `$layer.id`, PPI
- **Typed merges** (`precision_merge.rs`) render the members as a derived document. Merge Layers and
  Merge Down keep the **union extent** at its origin, as the byte merges do (`precision_merge_extent.rs`); Merge
  Visible is canvas-sized at (0,0) at both depths. The `merge_visible` floor counts
  `Content::Raster` only (host: `contributingRasterLeaves()`). Merge Down keeps the lower entry's id and label.
- **Typed locks** (`precision_lock.rs`) apply `doc_lock`'s four bits through `under_locks`, refusing with
  `"high-depth layer {i} is locked (…)"`. Writers that don't write the target's buffer only consult
  `lock_refusal`, and the host still owns the user-facing sentence.
- **`LLNK` (DRDC 47)** is conditional and stores u32 locks + u32 link per record. Reserved bits are masked.
- **`LCMP` schema 2, written at both depths**, binds comps by persisted `id` with no `layer` key, so comps survive
  reorder and rename. An unknown id makes the comp stale, not refused. Schema 2 without `LIDS` is refused.
- **`$layer.id` resolution** (`ActionPlayer.resolve`): the id alone within a session; across sessions only when
  that entry also carries the recorded name, else the name. *Why:* ids repeat across the files of a batch.
- PPI-only `set_resolution` is admitted at depth, and its buffers stay pointer-equal.

#### The measured Shift rule
- **For real keyboard events (`NSEvent(cgEvent:)`), a menu key equivalent matches its chord exactly.** ⇧⌘R never
  fires a ⌘R item, and ⌘R never fires a ⇧⌘R item. The Shift leniency 12D-2 inferred shows up only with
  hand-built `NSEvent.keyEvent` presses. So `ShortcutMenu.conflict`'s exact comparison is right, and ⌘Z/⇧⌘Z,
  ⌘S/⇧⌘S and ⌘E/⇧⌘E are not shadowed. Probe `shortcut-shift-rule-ui` pins both; the M1–M7 table is archived.

### 12E-1c: guides and ruler origin at depth (closes P7-26)
- **Shared guide mutators.** `doc_guide::{added, moved, removed, cleared, origin_set}` are wrapped by both depths;
  `geometry_chrome` moves guides on every typed canvas op, and a guide move alone counts as a change.
- **`LGDE` (DRDC 48)** is conditional and stores 9 bytes per guide, the byte v8 layout. A bad position is
  repaired; a bad orientation or count is refused. Guide ids are not persisted.
- `ffi_util::precision_op` is the one NULL-tolerant typed op wrapper, deliberately without a capacity gate.

### 12E-2: the slot rule and the typed opacity drag (P7-27)
- **One insertion rule: `core/src/layer_insert.rs`** (module doc = spec). `Above` = end of the anchor's subtree
  at its level, `Top` = root top, `IntoGroup` = one level into the anchor. Typed form `precision_insert.rs`;
  exports `dr_doc_adding_layer_at`, `dr_precision_doc_add_adjustment_at`.
- **A new entry joins a clip run** when the next sibling is clipped: an unclipped insert
  would silently re-base the neighbours.
- **Insertion is lock-exempt.** The UI inserts `.above`; `top` and `into_group` are MCP/Actions only.
- **`PrecisionOpacityDrag`** uses one capture and one builder for every preview and the commit. `finish` and
  `cancel` share a cancel-first prefix, and the panel reloads on every `ended` tick.

### 12E-2b: positional placement, typed `new_layer`, source locks
- **Every layer-creating path lands through the slot rule:** `SourceEdit::Place` (optional `anchor` +
  `position`, plan forecasts the positioned edit), `dr_doc_artwork_at` / `dr_precision_doc_artwork_at`, and
  typed `add_layer` (`layer_add`, admitted at 16-bit and float).
- **Typed source locks are consulted, not wrapped:** capture/replace → Merge (+ Position when the
  extent changes); transform → Position; rasterize → Pixels; place/retain ungated;
  `RawDocument::attachment_lock_refusal` → Pixels.
- **`JobProgressController` sets `animationBehavior = .none`.** The default `NSPanel` animation parked one
  thread per presentation, which led to a 64-thread deadlock.

### 12E-2c: Stamp Visible and Flatten at depth
- **`StampVisible { name, anchor }` / `Flatten {}`** use the document's own projection, with no export and no
  format change.
- **A stamp does NOT join a clip run** (the one slot-rule override), and it needs a contributing *raster* leaf.
- **A flatten drops all comps**, as the byte op does. It yields one `"Background"` with a new id and keeps
  channels, guides, profile, metadata, PPI and policy. It never answers a no-op. Both ops are lock-exempt.
- **Structure-edit variants are empty structs, never unit variants** (`MergeVisible {}`), because a serde unit
  variant ignores `deny_unknown_fields`. Merge/lock/link/stamp/flatten menu items are enabled at 16-bit and
  float with depth-aware floors.

### 12E-3: knockout, byte design D1–D12 (`core/src/style_knockout.rs`)
- **D1–D2 Keys.** `"knockout": none|shallow|deep` and `"blend_interior_as_group": bool` are style blending
  options, set via `set_layer_style` (no new export). Knockout ≠ none is non-identity; the flag alone is identity.
- **D3 Order.** Blend If reads the accumulated composite → Below effects → the interior unit against backdrop
  `B0` → remaining interior effects. The unit mixes by shape: `K = over(B0,C,q,M)`,
  `out = mix(acc,K,s·opacity)`, `q = fill·w`. Blend If is opacity, never shape; at `q = 1`, Normal equals the
  plain render (no fringe).
- **D4 Where the knockout stops.** Isolated group buffers and merge/private accumulators are hard stops (`B0`
  transparent). A clipped member punches to its clip unit's base-only snapshot. Shallow in a pass-through group
  punches to the accumulator at the group's start (root: Shallow ≡ Deep). Deep → the Background (D5), else clear.
- **D5 Structural Background.** Entry 0 qualifies if it sits at depth 0 and is a visible non-adjustment raster:
  Normal, opacity 1, no enabled mask, no style (and no Blend If at typed depth), at (0,0) and canvas-sized,
  with entry 1 unclipped.
- **D6 Interior as group.** When the flag is on, gradient/colour overlay, satin and inner glow move into the unit.
  On a clip base with visible members they follow the members inside the unit (P7-39 lifted the narrowing,
  2026-10-13: base at fill 1, members clamped to the footprint, then the grouped contributions).
- **D7–D9.** Flatten, Stamp and Merge Visible bake knockout exactly. Snapshots are charged to `RenderBudget`; a
  document without the keys allocates nothing and renders byte-identically. Keys are written only when
  non-default; lenient parse maps unknown values to `none`.
- **D10 Byte DRDC 49**, inside 45, has no table. It refuses `"invalid native knockout word"` and
  `"native knockout styles require DRDC 49"`.
- **D11–D12 Tolerances.** T1 = 1/255 vs the oracle; T2 = 2/255 vs Photoshop (4 for Blend If). Photoshop's `getd`
  returns neither key, so the gate reads the `knko`/`infx` bodies Photoshop wrote. A MISS is never widened.

### 12E-3b: typed twin, DRDC 50, UI, exact merges
- **`precision_style_knockout.rs` restates no rule.** Its only typed piece is
  `KnockoutSeed::{Projection, Handed}`; D5 never fires in a `Handed` (derived) document.
- **Typed DRDC 50** sits inside 46 after 48, with no table (keys ride in `LSTY`). It uses the same two refusals,
  checked even without `LSTY`.
- **(g) Exact merges.** Merge Down and Merge Layers use the real backdrop at 8 and 16 bits. **(vi) Limit:** exact
  only where `B0` is present and opaque; in an isolated group or at a root without a qualifying Background a hole
  bakes as transparency. **(iv)** A typed style is stored only if non-identity after removing Blend If.
- **(xi) Residual note.** `LayerStyle.knockoutMergeResidualNote` is asked by both `merge_down` branches *before*
  the edit, and covers R1–R3 plus R4 (a root with no qualifying Background, from 12E-3c). `isIsolatedGroup` is a
  conservative mirror of `doc_group::is_isolated`. The note stays silent when `B0` is present but non-opaque;
  it appears in MCP replies only.
- **(ii)** superseded: P7-39 lifted D6's narrowing and `clipBaseNote` was removed. **(iii)** At depth, `merge_visible`'s no-op names the raster floor.

### 12E-3c: PSD `knko`/`infx` carriage and D13
- **E1–E2 Codec (`psd_layer_effects.rs`).** `knko` = `[k,0,0,0]`, `infx` = `[0|1,0,0,0]`, read and written at both
  depths on rasters and opening folders (never closing dividers), after `iOpa`, `infx` first, before `lclr`.
  `clbl` is observation-only. Baked styles write neither and report a loss.
- **D13 / E3 (owner decision): no Background layer kind.** D5 stays structural, so a Deep knockout over an
  ordinary opaque bottom layer shows that layer where Photoshop shows transparency. Disclosed on import
  (`psd_group_records::structural_background_knockout`) and export (`document_reaches_background[16]`). A true
  Photoshop Background: lowest record, no `-1` channel, flags `0x09`, `lnsr bgnd`. Host mirror:
  `RasterDocument.hasQualifyingBackground`. Superseded at RGB 8/16 by P7-38's Background mark (open items).
- **E9** tracks `clbl` and `infx` separately. **E7** leaves `cmls` unimported (premise unverified). Pad
  handling in `psd_records.py` reads both Photoshop's 4-pad and Doctored's even pad.

### 12E-3d: the Photoshop measurement (closes P7-37)
- **Setup.** F1–F11 were fixed before the first Photoshop call. T3 is 2 8-bit-equivalent codes and the control
  floors are 32/8224. The 16-bit artefacts were derived from the committed `source.psd`, never re-authored.
- **Results** (no Rust or Swift changed): P16 ten MATCH; PD disclosed divergence at both depths, caused by D5
  alone; PX eight MATCH (Photoshop re-saves the bodies intact). Observation (a) `interior_on_clip_base` max 255 /
  mean 34 → P7-39, disclosed on PSD exchange. Observation (b): Photoshop's Merge Down punches to transparency;
  Doctored keeps the picture, by design.

## Rejected alternatives
- New outer words for `LLNK`/`LGDE`/50: would break the first-word and last-table invariants.
- A Background flag in 12E: D5 kept structural, divergence disclosed (D13); P7-38 later added the Background
  mark, which only a PSD sets, so a Doctored-made document keeps the structural rule.
- `!members.is_empty()` for D6's visible test: copies a byte deviation.
- Gating on Photoshop's `getd` read-back: 26.10.0 never answers it.

## Consequences
- **Typed Merge Layers / Merge Down keep the union extent**: the typed compositor renders any frame
  (`PrecisionScene::with_frame`); the one remaining difference is a masked adjustment member's outside coverage.
- **Knockout rules live once, in `style_knockout.rs`;** typed code, PSD code and host mirrors
  (`hasQualifyingBackground`, `isIsolatedGroup`) follow it, and the f64 oracle is extended before any rule change.

## Open items (verified 2026-09-26)
- `DONE` at RGB 8/16: **P7-38**, the Background mark (`doc_background`): PSD import marks record 0 Background or
  ordinary, an ordinary mark vetoes D5, RGB PSD export writes Photoshop's Background record, `.doctored` word 113
  (`LBKG`) at both depths; `deep_over_ordinary_bottom` matches at both depths. No UI sets a mark.
- `OPEN` [#31](https://github.com/kpgalligan/doctored/issues/31): the Background mark in the Gray, Lab, CMYK and
  32-bit float PSD codecs, which keep D13's disclosure ([the carryover ledger](phase-7-carryovers.md) P7-38).
- `DONE`: **P7-39** (2026-10-13), D6's clip-base narrowing lifted at both depths; the f64 oracle was extended
  first and `interior_on_clip_base` matches the `knockout_bottom` renders (max 2 / mean 0.5); the PSD disclosures
  and `clipBaseNote` are gone ([the carryover ledger](phase-7-carryovers.md) P7-39).
- `DONE` at RGB 8/16: **PSD `cmls` import/export** (`psd_comps`, `psd_comp_records`): resource 1065 and each
  record's `shmd`/`cmls`; a layer's settings ride in its own record, so an entry binds to the layer built from it.
  Comps that record visibility import as visibility comps, applying as Photoshop's take did
  (`core/tests/fixtures/psd_photoshop/layer_comps`, from layer-comps-521); export writes 1065, `lyid` and `cmls`.
- `OPEN` [#13](https://github.com/kpgalligan/doctored/issues/13): a comp's position and appearance in either
  direction (no authored sample of a moved layer, a changed style or a comment); `OPEN`
  [#31](https://github.com/kpgalligan/doctored/issues/31): comps in the Gray, Lab, CMYK and 32-bit float codecs.
- `DONE`: stale module docs in `core/src/precision_layer_blend.rs` and `core/src/style_knockout.rs` now point at
  the knockout implementation and P7-39 (2026-09-26).
- `DONE`: dead export `dr_precision_doc_group_all` / `groupingAll` / `layer_group_all` deleted end to end
  (`df7aa54`); tests group through `core/tests/common/group_roots.rs`.
- `DONE` (2026-09-29): byte Merge Layers keeps a visible clipped group that renders nothing
  as an empty group entry with the document's `visible` flag, so D6 matches the screen (12E-3b decision ix;
  `core/tests/knockout_merge.rs`).
- `DONE`: union-extent typed Merge Layers / Merge Down through a framed typed compositor (`precision_merge_extent.rs`, `core/tests/precision_merge_extent.rs`).
- `DONE`: float menu UI gate (`scripts/native-paint-probe/stampflatten.swift`); `merge_visible` floor wording
  (`app/Sources/AgentServer+Groups.swift`, header); 12E-2b drive single-instance (`576f69f`); comps survive profile
  edits (12E-2); `padded_*`, float canvas ops and the P7-39 predicate (12F-1/1b, `c83614f`); typed Paste (12G);
  multi-layer transform (`07d9153`).
- `DROPPED`: re-measuring shortcut pairs with real key-window enablement ("a strengthening of the probe, not an open decision").
