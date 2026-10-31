# Smart objects — built, then removed for v1 (ADR)

- **Status:** Accepted — closed; the features were removed 2026-10-10. Smart-object support is a later decision,
  not an open phase.
- **Phase / dates:** built 2026-10-09 to 2026-10-10 (last build with them: `3e05aea6`); removed in the batch
  after it.
- **Originals:** [archive/convert-to-smart-object-status.md](../archive/convert-to-smart-object-status.md) (the
  head, the Photoshop verdict tables, the phase plan, the open items) and
  [archive/convert-to-smart-object-shipped.md](../archive/convert-to-smart-object-shipped.md) (each shipped
  sub-slice's record). Both describe code that no longer exists.

## Context
Over two days the app gained smart objects, measured against Photoshop 26.10.0: Convert to Smart Object,
Edit Contents and its commit, New Smart Object via Copy, Replace Contents, Convert to Layers, Export
Contents, Convert to Linked, Embed, Relink, Update Modified and Update All Modified, Reset Transform, Stack
Mode, Package, layered `.rz` and PSD files placed as contents, all of it at 8, 16 and 32 bits, and PSD/PSB
smart-object read and write. They sat on two older features: placed images kept as retained sources
([Phase 8](phase-8-retained-source-status.md), [8F](phase-8f-host-source-status.md)) and live filters on those
layers ([Phase 9](phase-9-live-filters-status.md)).

## Decision
The owner's direction of 2026-10-10: v1 is a competent, well-rounded photo editor a professional might use.
Smart objects are not needed for that, and each sub-feature left a tail of unmeasured behaviour (the archived
status doc's open items). So the feature and what it rested on were deleted end to end; whether to build
smart objects again is decided after v1.

**Removed** (no FFI export, menu item, MCP tool, catalog entry, test, probe or drive stays behind for it):
- Smart objects: every command above, the Layer ▸ Smart Objects submenu, the contents window, and PSD/PSB
  smart-object read and write (`PlLd`/`SoLd`, `lnk2`, 32-bit contents as a depth-32 PSB).
- Live filters on placed and RAW layers, their previews, caches and the filter-preset library (Filters ▸
  Convert for Smart Filters, Live Filters…, Filter Presets…, Remove Live Filters; `DR_FILTER_PRESETS_DIR`).
- Non-RAW retained sources: a placed PNG, JPEG, TIFF, HEIC, JXL or movie frame, a linked placed file, and the
  Live Photo layer's frame selection (the `live_photo` described kind, Image Source…, Select Live Photo Frame…).

**Kept:** camera RAW layers and the retained-source core they run on (`source_asset.rs`, `retained_source*.rs`,
`SourceEdit::{Retain, Place, Transform, CaptureRaster, Rasterize, Replace}`, the `RBIN` pool, RAW local masks):
open, Place Embedded/Linked of a camera file,
Redevelop, RAW presets, Synchronize, RAW locals, Migrate decoder, at 8, 16 and 32 bits. The destructive
Gaussian Blur, Unsharp Mask, High Pass, Reduce Noise and Dust & Scratches keep the kernels the live filters
shared, now `core/src/filter_kernels.rs`. MCP keeps `place_image`, `get_image_source`,
`rasterize_image_source`, `add_live_photo_layer` and every RAW tool.

## What a place does now
- Place Embedded, Place Linked and `place_image` of a non-camera file land an ordinary raster layer at the
  document's depth: the core's own `place` (so the colour conversion and depth rule are unchanged), any
  print-size transform, then `rasterize` of the landing index, as one edit and one undo step. `storage` matters
  only to a camera file, whose RAW layer keeps the link.
- File ▸ Open of host media (HEIC, JXL, a movie frame) keeps its decode, its 16/8-bit choice and its metadata
  pass, and opens as one raster layer. Place Live Photo… and `add_live_photo_layer` land the pair's still, or
  the chosen movie frame, the same way.
- Layer ▸ Rasterize RAW Layer (`rasterize_image_source`) turns a RAW layer into its current pixels; it refuses
  a layer with no camera original.

## File compatibility
- **Native files (`.doctored`, legacy `.rz`).** The removal did not bump `DRDC_VERSION` (73 at the time) and
  the reader accepts every earlier version. Words 65/66 (contents), 68
  and 73 (float contents), 69/70 (Stack Mode), the `SOCT` and `SOSM` tables and the live-filter records
  (`LFLT`, with its mask references still range-checked) are bounds-checked and skipped, never decoded. A
  binding whose record carried any of them is dropped and the layer keeps its stored pixels. A plain or linked
  placed binding is kept by the core and rasterized by the app's open pass (`ImageDocument.read`, native files
  only, before the history resets), which also clears `live_photo` meta. No prompt, no undo step: the layer
  opens with the pixels the file last composited. The writer no longer writes any of those words or tables;
  its output for every other document is byte-identical, and its caps are unchanged.
- **PSD.** A `PlLd`/`SoLd` record imports as its layer pixels, as before phase 8a. Layered PSD export writes
  every layer, RAW included, as pixels.
- **Not covered:** a recovery or snapshot archive written before the removal; a non-RAW binding in one is not
  rasterized on restore.

## Profile conversion with RAW layers
With placed sources gone, the retained-source refusal of Convert to Profile applies only to RAW layers
(`ConversionOptions::source_refusal` in `doc_color_policy.rs`, its one home). A source re-renders relative colorimetric with BPC off, so
between matrix/TRC profiles relative, perceptual and saturation keep RAW sources bound (all three give the
source render's bytes), and with a LUT profile on either side only relative does (at 16 bits a LUT side still
refuses at the typed colour plan); black point compensation and absolute refuse everywhere. Past any of these
refusals, `rasterize_sources` (the Convert sheet's "Rasterize N RAW layers, then convert", MCP `convert_profile`,
recorded in Actions) rasterizes every bound layer and converts as one result. No intent is persisted per source.

## Evidence kept
- `core/tests/fixtures/rz_removed_features/`: 18 `.rz` files written by `3e05aea6` (a smart object, float
  contents, Stack Mode, three live-filter cases, a linked and an embedded placed layer, a RAW layer, each byte
  and typed) with each file's stored layer pixels beside it. `core/tests/removed_feature_files.rs` opens every
  one and compares the pixels exactly.
- `core/tests/psd_smart_object_pixels.rs`: a Photoshop smart-object PSD imports with no retained source and
  flattens to the file's merged composite. `core/tests/fixtures/psd_photoshop/smart_objects/` keeps only the
  four files it reads.
- `core/tests/convert_rasterize_sources.rs`: the refusal without `rasterize_sources`, and with it a result
  equal to rasterize-then-convert.
- The Photoshop takes stay in `~/devel/doctored-fixtures/photoshop/` (outside the repo, never committed):
  `convert-smart-object-158/` take1 to take9-3, `float-edit-contents-600/`, `float-smart-object-files-600/`,
  `float-layered-place-600/`, `float-low-depth-600/`, `float-contents-low-depth-600/`,
  `float-contents-layers-600/` and `float-contents-kinds-600/`. Their runners and the `convert_smart_object`
  job in `photoshop.jsx` were deleted; the archived status doc names each runner and take, and `3e05aea6` has
  the code.

## Consequences
- A document cannot keep a placed image's original: scaling a placed layer down and up again resamples
  pixels, as for any raster layer. A camera file is the exception.
- A filter is destructive. Non-destructive tone work stays with adjustment layers.
- A PSD round trip flattens Photoshop's smart objects to pixels, in both directions.
- An old `.rz` loses its smart objects, live filters and placed originals the first time this build saves it.

## Open items
None for the removed features. The backdrop filter layer was deferred before the removal and is not
revived by it.
