# Relative placement of ordinary PSD raster masks (ADR)

- **Status:** Accepted — decision reversed 2026-10-11; see "Decision in force"
- **Phase / dates:** 10A relative-mask slice, 2026-09-13; Photoshop measurement Run 3, 2026-09-16; reversal 2026-10-11
- **Original:** [archive/phase-10a-relative-masks-status.md](../archive/phase-10a-relative-masks-status.md) — full history, measurements, receipts
- **Key commits:** `1992d43` (slice), `d1b6c27` (masks measured against Photoshop)

## Context
The Adobe spec defines mask-flags bit 0 as "position relative to layer". The [ordinary mask
slice](phase-10a-masks-status.md) treated such masks as losses. The 10A slice admitted them on raster records
by normalizing the rectangle into canvas space once. Run 3 then measured that Photoshop itself ignores the
flag, which left the question of which reading to keep.

## Decision in force (2026-10-11)
- **The stored rectangle is read as canvas coordinates, on rasters and folders alike. Bit 0 is recorded
  and never applied**, which is how Photoshop reads a file that sets it. `MaskRecord::classify`
  (`core/src/psd_raster_masks.rs`) no longer takes a raster origin and has no normalization; the flag stays
  inside the admitted-flags whitelist and only chooses the report phrase: `absolute placement` when clear,
  `placement at its stored rectangle (the PSD position-relative flag, bit 0, is set and not applied, as
  Photoshop reads it)` when set.
- A folder mask with bit 0 set is therefore an ordinary admitted mask, not a loss.
- The 16-bit and 32-bit readers share `MaskRecord`, so they changed with it. Export (bit 0 always clear)
  and `.rz` are untouched.
- Result is still `linked=false`, exact samples, outside coverage, enabled state; density 1, feather 0. A
  flag-set file and its flag-clear twin with the same stored rectangle open as the same document and save
  the same native bytes.
- Bit 0 combined with an unsupported form (extended body, vector/`-3`, rendered/inverted/unknown flags,
  parameters, unsupported codec) remains an undecoded disclosed loss.
- No preference for the other reading: Photoshop never writes the flag, so there is no file a user could
  make that depends on it.

### Photoshop appearance agreement (Run 3, 2026-09-16)
- Photoshop 26.10.0 writes every mask **absolute** (bit 0 clear) and trims the stored rectangle to the
  mask's nonzero extent. Its masks render in Doctored at 0 codes (fixture
  `core/tests/fixtures/psd_photoshop/masks/`, read by `core/tests/psd_photoshop_constructs.rs`).
- **Photoshop ignores bit 0** and reads the rectangle of the byte-patched twin (`source_relative.psd`)
  where it stands (receipt field `photoshop_relative_reading: "absolute"`). Since the reversal Doctored
  reads the twin the same way: each mask sits at its stored rectangle, so the twin is a different picture
  from `source.psd` and equal to a control whose rectangles carry the same move with the flag clear.
- Reverse direction: Doctored's unlinked masks exported via `export_psd` keep canvas-space rectangles
  (bit 0 clear) while the layer moves, and render in Photoshop at 0–1 codes at both depths.
  `add_layer_mask from_selection` yields a canvas-sized mask at both depths.
- `scripts/verify_phase10_photoshop_appearance.py --receipt A5` holds the gate (`M3_twin` now requires
  the twin to equal the flag-clear control and differ from the original); it was edited with the reversal
  and not re-run, because a run drives Photoshop.

## Superseded
- The 2026-09-13 decision (normalize once against the owning raster's original top/left; a relative
  folder mask is a loss) and the 2026-09-16 "keep Adobe's documented reading" decision. Their 44-case
  absolute/relative pair corpus (`core/tests/psd_relative_masks.rs`) and its app verifier
  (`scripts/verify_phase10a_relative_masks.py`) were deleted with them.

## Rejected alternatives
- Folder bounds / descendant union as an origin for a relative folder mask — ambiguous, and moot now.
- Treating bit 0 as a native content link — the spec separates relativity from linkage.
- A preference choosing between the two readings — nothing writes the flag.

## Consequences
- Tests: `core/tests/psd_raster_masks.rs` (a bit-0 raster mask at 8 and 16 bits against its flag-clear
  twin: layers, flattened pixels and native bytes equal; admitted case `folder_relative_flag_mask`) and
  `core/tests/psd_photoshop_constructs.rs` (`the_relative_twin_reads_as_photoshop_does`). The 53-case
  mask corpus counts 15 ordinary, 20 losses and 18 refusals.

## Open items
- `DONE` — "Relative-mask semantics vs Photoshop": Photoshop's reading, 2026-10-11 (above).
- `DROPPED` — Extended/vector/parameter forms: explicit exclusions of [phase-10a-qualification.md](phase-10a-qualification.md).
- `DONE` — Broader depth/compression: 16-bit + 16-bit ZIP masks ([phase-10c-depth-status.md](phase-10c-depth-status.md), `core/src/psd_zip.rs`).
- `DONE` — Independent-application exchange: GIMP closeout (`352cb50`) and Photoshop Run 3 (`d1b6c27`).
- `TRACKED` — 9I prerequisite gate (mentioned as unchanged) → plan §9I.
