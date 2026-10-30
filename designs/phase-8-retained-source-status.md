# Phase 8 — retained sources: embedded, linked, RAW and Develop (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 8A–8D (this record; 8E/8F/8G have their own ADRs), 2026-09-10 → 2026-09-11
- **Original:** [archive/phase-8-retained-source-status.md](../archive/phase-8-retained-source-status.md) — full history, measurements, receipts
- **Key commits:** `3e0d2dd` (8A), `a5868ab` (8B), `6e611fc` (8C), `943dbf5` (8D); siblings `23cf25d` (8E), `9b3f65c` (8F), `e386448` (8G)
- **Removed 2026-10-10:** placed images as retained sources (8A embedded, 8B linked). Place Embedded, Place
  Linked and `place_image` of a non-camera file now land an ordinary raster layer; Replace Contents, Refresh,
  Relink, Embed, Update Modified Content, the portable copy and live filters on a binding are gone, for camera
  files too. Camera RAW layers (8C, 8D, [8G](phase-8g-raw-enhancements-status.md), and camera files placed with
  Place Embedded/Linked…) keep the source model this record describes, with Layer ▸ Rasterize RAW Layer as its
  rasterize command. An old `.rz` opens a placed layer as its pixels. See
  [the removal ADR](convert-to-smart-object-status.md).

## Context
Before Phase 8 every placed image, RAW development or scaled layer was baked into its layer's pixels, so
repeated transforms, profile changes and RAW re-edits accumulated loss and a display image could silently
become the working master. Phase 8 makes an original *source* a first-class, immutable, persisted asset that
the layer's pixels are re-derived from, in both document depths, while keeping a usable rendered fallback so
saved work opens without the original file or decoder. It closed P7-01, P7-02 and P7-35 (P7-03–P7-05 via
[8E](phase-8e-artwork-status.md)/[8F](phase-8f-host-source-status.md); [8G](phase-8g-raw-enhancements-status.md)
completed the phase).

## Decisions
- **Immutable source asset, separate from the layer fallback.** An asset keeps the decoded,
  orientation-normalized straight RGBA master at its *actual* depth (a byte master stays byte in a 16-bit
  document) plus its original RGB ICC, under a persistent path-independent ID and a content revision. The
  layer stores the accumulated affine, interpolation policy and a rendered fallback in the document profile.
  *Why:* re-rendering from the original is the only way scale-down/up and profile conversion stay lossless.
  Code: `source_asset.rs`, `retained_source.rs`, `doc_retained_source.rs`, `precision_retained_source.rs`.
- **Fixed evaluation order:** original → document-profile conversion → (live filter stack, added in
  [Phase 9](phase-9-live-filters-status.md)) → accumulated affine sampling → explicit fallback-depth conversion
  → layer mask → styles/blending/opacity. Byte originals promote (×257) before typed evaluation; typed originals
  render at 16 bits before one nearest, undithered byte quantization. Identity placement restores the original
  samples exactly when profiles/depths agree. `retained_source_render.rs` (`work_plan` shares the render's
  exact/permutation/filtered sampling decision so admission matches rendering).
- **A display CGImage is never a master; no implicit reinterpretation.** Convert Profile re-renders from the
  original; Assign Profile on a source-bearing document refuses (11C later added the disclosed
  `assign_profile_rasterizing` path). Destructive pixel edits require the explicit **Rasterize** (Layer ▸ Smart Objects)
  step, which detaches the source keeping current fallback pixels; Undo restores the binding. Core
  fallback-allocation/profile guards stop a stale caller from resurrecting a source after its fallback changed.
- **Linked masks follow the incremental transform** with the established mask sampling rule; a separately
  edited mask gets no immutable master (explicit non-goal).
- **Native persistence (all older writers/readers stay; source-free files keep v10/v13):**
  byte v14 / typed v15 embedded sources (`RSRC` table, lossless PNG at native depth, one copy per referenced
  asset); v16/v17 linked/advanced source records; v18/v19 retained RAW (deduplicated `RRAW` table for camera
  originals, `native_sources_raw.rs`). All records, references, dimensions, profiles and aggregate capacity
  are preflighted before any pixel decode; malformed native files never fall through to an image decoder.
  Source ICC: 16 MiB each / 64 MiB aggregate. RAW originals: 256 MiB each / 512 MiB distinct aggregate;
  recipe descriptor 64 KiB. Ordinary embedded sources require an empty layer-description slot (writing other
  metadata detaches the source).
- **Linked assets (8B):** bindings record an absolute local path + SHA256 of a private snapshot taken before
  decoding; cached originals/profiles and fallbacks are always saved. File changes never auto-rerender:
  Refresh is explicit (one undoable revision; unchanged is a no-op), Relink accepts only the same captured
  bytes, different bytes need Replace Contents (new revision, keeps identity/affine/properties). Embed works
  from the cache even with the file missing. Save Portable Image Copy writes one native `.rz` with links
  removed (decoded originals, not original containers); it is MCP-only and excluded from Actions like
  `save_copy`. Cross-depth placement/replacement refuses by default and offers explicit conversion.
- **Retained RAW (8C):** the exact camera file is embedded beside a genuine u16 SDR developed master and the
  fallback; a versioned dense recipe captures every effective control, exact as-shot chromaticity,
  orientation, output policy and decoder/OS/Core Image identity. Redevelop (**Layer > Redevelop RAW…**,
  `redevelop_raw`) patches saved values below existing retouch layers; unsupported controls refuse, unchanged
  requests are no-ops. Opening saved work never runs a RAW decoder; changed/unavailable decoders or unknown
  recipes keep saved pixels and refuse redevelopment. Production rejects missing/non-opaque camera output
  (sandbox-denied Core Image can return zero-filled buffers without error).
- **Develop workflow (8D):** full-resolution u16 inspection (Fit ≤1600 px, 100% region ≤1024 px), histograms
  counting every alpha-positive pixel with exact 0/65535 endpoint counts, clipping markers OR'd across Fit
  cells. Readouts precede affine/masks/retouch. Output space sRGB or Display P3 (recipe v2; v1 stays sRGB).
  **Adobe RGB (1998) and ProPhoto RGB** join them with no recipe, preset or DRDC bump. Both
  profiles are the core's own built-ins from the published primaries (`icc_builtin.rs`: gamma `curv` 563/256
  and 461/256); no profile file is read. ProPhoto is the pure 1.8 power, not ISO ROMM with its linear toe,
  and Core Image renders it into the named `CGColorSpace.rommrgb` (`RawOutputSpace.cgSpace`). A colour
  outside the chosen space is clamped per channel to 0 or 65535; neutral highlights clip alike in every
  space. Drive: `scripts/verify_v1_raw_output.py`.
  Presets store selected user controls only, never decoder defaults or camera identity (128 × 8 KiB,
  `DR_RAW_PRESETS_DIR`, atomic writes, advisory lock); sync ≤32 targets, all prepared before any document
  changes. Actions record resolved `apply_raw_settings`, never preset names. **Decoder migration is explicit
  and reviewed** (`preview_raw_decoder_migration` ticket: one app-wide, 5-minute expiry, released on close;
  `migrate_raw_decoder` commits exactly the reviewed pixels); it refuses lost authored controls, changed
  geometry or unknown semantics.
- **Recipe numbers are persisted as exact binary64** — `serde_json` correctly-rounded float parsing is
  required; the default parser moved values by one ULP and broke reviewed-descriptor equality.
- **All edits** capture document/target/selection and publish through the shared worker and one central
  Undo; stale results never publish. Admission counts the original even when the fallback is tiny (an
  estimate, not an RSS ceiling). Resource inventory deduplicates shared masters across document/history.

## Rejected alternatives
- Retaining original compressed containers for ordinary images — decoded masters suffice; portability packs decoded data.
- Base64 source/RAW payloads in layer metadata — binary native tables with preflight instead.
- Auto-refresh or silent redevelopment/reinterpretation on file or decoder change — explicit, reviewed actions only.
- Implicit byte promotion / 16-bit reduction on placement — explicit cross-depth choice instead.

## Consequences
- Any new destructive or geometry path must either re-render from the source or go through explicit
  rasterization (`RetainedImagePolicy.swift`); never write over a bound fallback.
- New native tables must preflight before pixel decode and keep older writers selected when absent.
- Retouch lives on *other* layers above a RAW layer; redevelopment must never touch them.

## Qualification
8A: fixed 96×64 fixtures in both depths with independent bilinear and Display P3→sRGB references (sub-byte
u16, tiny alpha, latent RGB) compared on every fallback sample through 1/8 scaling, relocation, source
deletion, profile conversion and return to full size. Rust, host, Python and external MCP groups all passed;
no large-document performance claim (Phase 7's 24 MP result stands).

### Phase 8B qualification
All four master/document depth combinations; 28 cached originals and 16 fallback/profile arrays checked by
independent readers; moved/changed/missing/unreadable links, refresh no-ops, portable copies reopened after
deleting originals. Capacity boundaries (1,024 layers, 400 M weighted source/fallback pixels) are tested with
small shared buffers — capacity tests, not 400 MP workflows.

### Phase 8C qualification
A CC0 3008×2000 Nikon D70s NEF ([fixture manifest](../scripts/raw-probe/camera-fixture.json)) plus synthetic
DNGs: master components vs an independent Core Image reference within a fixed 3-code bound (max 1); exported
composites exact; retouch pixels exactly match shifted camera codes; exposure round trips exact.

### Phase 8D qualification
Four full masters in Display P3 within the 3-code bound (max 1); 15 IEEE bit patterns survive eight save/open
cycles; presets, sync, stale/expired migration tickets and fresh-process reopen pass in both fallback depths.
Reproduction: [`scripts/raw-workflow-probe`](../scripts/raw-workflow-probe/README.md).

## Open items (verified 2026-09-26)
- `TRACKED` — Deduplicating masters across separate snapshot/recovery generations; compressed native/archive payloads → carryover P7-32 / plan §13C.
- `TRACKED` — Noncancellable RAW capture/decode, source snapshot copy and fingerprint stages → P7-32 / §13C.
- `TRACKED` — Larger source-heavy/multilayer/history workloads, encoded-input budgets, broader camera corpus and migration coverage → P7-33 / §13D.
- `DONE` — Floating/HDR RAW development output: the 32-bit float (linear) development (`RawOutputEncoding.linearFloat`) keeps an f32 master beside the camera file in a 32-bit float document, under typed DRDC word 78 (`float_native`); it never narrows into an 8- or 16-bit document. ADR: `phase-8g-raw-enhancements-status.md`.
- `DONE` — Typed RAW capture packets/PPI (11D typed metadata bridge, `core/src/precision_metadata_edit.rs`; plan §11D).
- `DONE` — Typed camera auxiliary matte/depth import (9G, P7-08 verified).
- `DONE` — Source-preserving Assign decision: refuse + disclosed `assign_profile_rasterizing` (11C; `app/Sources/AgentServer+Color.swift`).
- `DONE` — Retained-source whole-document, group/linked and perspective geometry (12F-2, `07d9153`, `RSPJ` table); geometry-changing RAW migration with local-plane remap (12F-4, `6400648`).
- `DONE` — Typed layer locks / arbitrary placement for sources (12E-2b, P7-26/P7-27 verified); retouch improvements (9D).
- `DONE` — Per-entry RAW preset repair (12B: `broken` rows, `repair_raw_preset`, schema 2).
- `DONE` — A placed or replaced ordinary source keeps the core open's profile refusal on its immutable asset (`SourceAsset.profile_refusal`), reported by `get_image_source` and Layer > Image Source…, and persisted in `.rz` by conditional word 56 (byte) / 57 (typed) inside 45/46 and the `RSPR` table before `LIDS` (`core/src/source_refusal_native.rs`).
- `DONE` — UI Place Embedded/Linked, Replace Contents and Refresh show that refusal once, after the edit commits, as an informational "Embedded Colour Profile" sheet carrying the MCP `disclosures` sentence verbatim (`EditorViewController.presentSourceProfileNotice`). `RetainedImageWork.edit` now reports whether it published, so a no-op import returns no refusal and MCP stops disclosing it. Verified by typecheck, build and review: the UI path opens an `NSOpenPanel`, which automation cannot drive.
- `DONE` — Place Embedded/Linked…, Replace Contents, Update Modified Content and `place_image`/`replace_image_source` of a camera RAW file land a retained RAW layer at 8, 16 and 32 bits (`RawSourcePlacement`): the u16 development is the master under `convert_fallback` at every depth (no consent, as the open's 8-bit choice), the camera file is attached, a place is scaled by document ÷ file ppi through a non-interactive `transform` (a place request's own `source_transform` is held to `MAX_WORK_UNITS`), and the `.xmp` sidecar's `Exposure2012` applies (open too; other settings disclosed). Redevelopment keeps a linked layer's link. Refresh carries the saved development and refuses locals/enhancements; Update All still skips `camera_raw`. Admission (24 MP, `RawSourcePlacement`'s record): every stage ≥ 1.25×, no constant moved. Tests `raw_place.rs`; drive F36.
