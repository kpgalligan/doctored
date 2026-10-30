# Per-construct PSD exchange rules (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 10C-2 slices 1–3 plus closeout Runs 1–5, 2026-09-14 → 2026-09-17 (knockout carriage added by 12E-3c, 2026-09-20)
- **Original:** [archive/phase-10c-constructs-status.md](../archive/phase-10c-constructs-status.md) — full history, per-kind and per-effect tables of agreement measured against Photoshop, render SHA-256s, receipts
- **Key commits:** `75ab10e` (slice 1), `9b6a8aa` (slice 2 phase 1), `1544fdf` (slice 2 phase 2), `fc70079` (slice 3), `502cc88` (receipts R1–R8), `693188b` / `d1b6c27` / `5e0ed4d` (Photoshop Runs 2–4), `fa336cf` (Run 5), `1063561` (12E-3c knockout)

## Context
A layered PSD holds Photoshop constructs Rasterize can only partly model: adjustment/fill records with no
pixels, layer effects, fill opacity, Blend If, group clipping, text/shape/smart objects. Before 10C-2 several
of these refused the whole layered import or were silently dropped. The slice fixes ONE rule per construct,
for both directions (import/export) and both depths (8-bit byte documents, 16-bit typed documents), and
requires every non-"preserve" outcome to be a sentence in the import/export report (the compatibility sheet,
and `inspect_psd` / `inspect_psd_export` for the model).

## Decisions
- **The rule table** (the contract `GENERAL_LIMITATIONS` in `../core/src/psd_import.rs` and the export preflight summarize):

  | construct | import (8 / 16-bit) | export (8 / 16-bit) |
  |---|---|---|
  | layer style (effects `lfx2`, fill opacity `iOpa`, Blend If ranges) | PRESERVE as one Rasterize style on a raster or folder + observation; an unrenderable body is DISCARDED + loss with its reason. 16-bit also discards a style on a Pass Through folder or adjustment record, and resolves the file's global light into each effect | PRESERVE as `lfx2` + `iOpa` + 40-byte ranges + observation; only what Photoshop's dialog / a PSD mask cannot hold is RASTERIZED (baked) + loss with the reason; a group whose style must bake REFUSES |
  | adjustment record the codec reads (`nvrt thrs post levl curv blnc hue2 phfl expA mixr` binary; `vibA blwh` descriptor) | PRESERVE as an adjustment layer (mask, opacity, blend key, clipping) + observation; unmapped body OMIT + loss with reason | written as that record + observation |
  | any other adjustment / fill record (`SoCo`, `brit`, …) | OMIT + loss | REFUSE naming the op (e.g. `bcs`) |
  | empty record (no pixels, no adjustment key) | canvas-sized transparent raster + observation | written as pixels |
  | text / shape / smart object / retained source | pixels + marker loss | pixels + loss |
  | group clipping | PRESERVE (`Layer::clipped`, `PrecisionDocumentLayer::clipped`) | written |
  | Blend If ranges | PRESERVE iff exactly one of gray/red/green/blue is restricted and every other pair is default; else loss | written as the record's 40-byte ranges |

- **Report sentences are fixed strings that tests match.** Import omission/empty/carry/decline, the typed
  global-light observation (once per file), and export preserve/bake/refuse sentences live in the importers and
  `psd_export_preflight.rs`; their exact text is in the archive. The export refusal order is fixed: canvas side →
  file size → per-entry/mask side → baked style past 100 MP → adjustment op → group style that must bake → mask
  density/feather.
- **`GroupRecords::retains_nothing` drives both the report and the `open`/`open16` branch**, so a file of only
  omitted records reports and opens as its stored composite — the report can never advertise a layered choice the
  open refuses. A both-sides-zero rectangle is a pixel-less record (omitted or empty layer); pixel-less framing is
  exact (one stray plane byte still refuses "length mismatch"). Empty records are canvas-sized because byte painting
  never grows a layer.
- **One codec per construct family.** `../core/src/psd_adjustment.rs` is the only adjustment body parser/writer
  (both directions, both depths; `KINDS` is twelve entries; its module doc is the byte-layout and mapping spec, with the
  `mixr` body in its private half `../core/src/psd_adjustment_mixr.rs`, measured against Photoshop-written files);
  `typed_description` is the only byte-form → typed adapter. `../core/src/psd_descriptor.rs` is framing only
  (version 16; bounded: nesting ≤ 16, counts ≤ 4096, every length checked; `obj `/`ObAr`/unknown OSTypes decline).
  `../core/src/psd_layer_effects.rs` + `psd_layer_effects_fx.rs` own `lfx2`/`iOpa`/ranges; that module doc is the
  complete key mapping table.
- **Adjustment carry details.** `expA` is a BINARY body (Adobe's table), imported with `"evaluation":"document_v2"`
  so it evaluates identically at both depths; export ignores the key. `phfl` v2 colour space 7 (Lab, signed
  hundredths) is carried through `lab::lab_to_srgb8` then the file's sRGB→document transform; other spaces decline;
  export writes space 0. `curv` pads to even length. Admission charges a carried record like an empty canvas-sized
  raster. A typed adjustment carries the same blend keys a raster does (v37, [adjustment blending](phase-10d-adjustment-blending-status.md)).
- **Style carry details.** Import merges the three constructs into ONE `LayerStyle` via
  `LayerStyle::from_value(_, Strict)` and clears exactly the markers it carried (`FeatureMarkers::carry_style`); an
  identity result or a body that is not an effects descriptor leaves the marker sentence byte-for-byte. Stacked
  instances (`lmfx`) decline; `lrFX` beside `lfx2` is the legacy duplicate and does not block. Style colours are
  authored sRGB and convert with `Transform::color` in both directions at both depths.
- **Export writes `lfx2` only when `LayerStyle::has_enabled_effects()`** (four sites plus the cost estimate; the
  import side stays on `effects.is_empty()`); `iOpa` only below 100 %; image resources 1037/1049 iff an `lfx2` is
  written (typed documents write 120°/30°). Photoshop itself surfaces no `layerEffects` for an effect-less block.
- **The bake** (`psd_export_rasterize::rasterized_style`) composites the layer into
  `style_composite::merge_extent` through the shipped compositor at opacity 1, Normal, no Blend If — those belong
  to the record's own opacity byte and blend key. An ENABLED mask is inside the bake (not written again); a disabled
  one is written as an ordinary mask and still faces the density/feather refusal. The preflight forecasts the same
  rectangle (`RzDocument::style_extent`, pixel-free) for the 100 MP and 2 GiB checks. Bake reasons: stroke < 1 px,
  satin distance outside 1–250 px, enabled mask with density or feather.
- **Photoshop-written PSD tolerances (Run 2):** the 8-bit layer-info section may carry up to three pad bytes; an
  empty-rectangle mask at outside 255 is "no mask" (at outside 0 a reported drop); the "pixels irrelevant" marker is
  read only on a record with a rectangle. Photoshop's 16-bit records in the `Lr16` tail with ZIP(-prediction) layer
  and mask planes import layered since Run 4 ([depth status](phase-10c-depth-status.md)).
- **Non-goals, fixed:** no per-export choice between preserve/rasterize/refuse; `TySh`, vector/shape and
  smart-object payloads rasterize; PSB, CMYK/Lab and 16-bit grayscale stay refused; contours, noise, technique,
  texture, pattern, jitter, glow range, `lrFX` and `clbl` are not interpreted (disclosed in the import sentence).
- **Agreement is measured, not promised.** Carried adjustments and written effects say "mapped onto Rasterize's /
  Photoshop's own arithmetic … can differ / not qualified". Closing a measured gap means changing Rasterize's own
  arithmetic or an effect kernel — a separate per-kind decision, not part of this exchange contract.

## Rejected alternatives
- Refusing the whole layered import on an unreadable adjustment record — it hid valid layers; omission with a loss replaced it.
- Writing a separately enabled mask beside a baked style — it would apply twice.
- Carrying `phfl` v3 (XYZ) — Photoshop 26.10.0 never writes it; v2/space 7 was the real form.
- An effects descriptor with no enabled effect — reopens as a bare marker and Photoshop ignores it.

## Consequences
- Any new Photoshop construct must get a row in this table and a sentence, never a silent drop; new adjustment
  kinds go through `psd_adjustment.rs` only, new descriptor classes through `psd_layer_effects*.rs` only.
- Committed goldens `../core/tests/fixtures/psd_10c/*.psd` pin an unstyled document's bytes; Photoshop-authored
  fixtures under `../core/tests/fixtures/psd_photoshop/` (`adjustments`, `blend_if`, `masks`, `depth16`, `adjustments_channels`) pin import.
- Suites: `psd_constructs.rs`, `psd_adjustments.rs`, `psd_adjustment_descriptors.rs`, `psd_layer_effects.rs`,
  `psd_disabled_effects.rs`, `psd_photoshop*.rs`; `RZ_PSD_CONSTRUCT_FIXTURES=<dir>` writes the construct PSDs.
- Re-runnable harnesses: `../scripts/verify_phase10_receipts.py`, `../scripts/verify_phase10_photoshop.py`,
  `../scripts/verify_phase10_photoshop_appearance.py`, `../scripts/verify_phase10_photoshop_depth16.py`
  (Photoshop 2025 26.10.0 pin in `../scripts/photoshop-probe/`).

## Open receipts (ledger)
None open. The four manual receipts once listed here (R1 binary-body adjustment carry, R2 descriptor adjustment
carry, R3 `lfx2`/`iOpa`/Blend If carry with its bake case, R4 GIMP 3.2.6 reopen of the `styled_effects` corpus
case) were all taken 2026-09-16 in `502cc88` via `scripts/verify_phase10_receipts.py --receipts R1|R2|R3` and
`scripts/psd-exchange-probe/run_reopen.py` + `verify_phase10b_external.py` (GIMP files committed under
`../core/tests/fixtures/psd_10b/styled_effects/`); artifacts and SHA-256s are in the archive. GIMP attaches no
filter from `lfx2`, so that projection is recorded, never gated.

## Open items (verified 2026-09-26)
- `OPEN` — Part C phase 2, drawing a map curve with the pencil in the Curves dialog (plus Smooth and points→map, in all three dialog modes), is the open item. A1/A2 done 2026-10-02: Photoshop 2025 26.10.0 authored `../core/tests/fixtures/psd_photoshop/adjustments_channels/` (`../scripts/verify_phase10_photoshop_channels.py`, all five gates passed). Measured: `levl` is 632 bytes — records 0–3 composite/red/green/blue, then an `Lvls` v3 tail of 33 identity records; a point `curv` is `is_map` 0, v1, bitmask of only the given channels (composite + red = 3), then a `Crv ` v4 tail; a `Mpng` curve is `is_map` 1, v1, bitmask 1, 256 map bytes, then a `Crv ` v3 tail; Photoshop applies the per-channel stage BEFORE the composite one for both Levels and Curves (within 2.53 / 1.19 codes; the other order misses by 43.5 / 83.7). `DONE` B: per-channel and output Levels carry in that order — `levels` gains `output_black`/`output_white` and `r`/`g`/`b` stages (`../core/src/adjust_levels.rs`), `levl` records 0–3 decode and encode both ways (only the version, ceiling and gamma checks still decline), `.rz` conditional words 61/62 (`levels_native.rs`), the Levels dialog in all three modes (`LevelsAdjustmentEditor.swift`) and MCP; the legacy single-purpose Levels export is deleted. `psd_photoshop_channels.rs` now carries all three Levels cases and gates each against Photoshop's own render at a tolerance of 3 codes fixed before the run (measured worst: `levl_channels` 1 / 0.04, `levl_both` 3 / 1.33, `levl_output` 0 / 0.01 codes at 8 / 16 bits). Auto Tone / Auto Color join the Levels layer dialog and MCP `auto`, measured over the backdrop below the layer with Photoshop's scope (`../scripts/verify_levels_auto_photoshop.py`, 26.10.0): the whole visible backdrop counts whether the layer is clipped, inside an isolated group or under a live selection; only the layer's own mask (or a new layer's selection) weights it. `DONE` C phase 1 (2026-10-03): the `curv` map form (`is_map` 1) carries — `curves` gains `rgb_map`/`r_map`/`g_map`/`b_map` (exactly 256 integers 0…255, kept verbatim because pencil maps are non-monotone; never mixed with point keys), applied channel first then composite, exactly `rgb_map[ch_map[v]]` at 8 bits and as f64 piecewise-linear maps at 16/32 (`../core/src/adjust_curves.rs`); import reads every set bit's 256 bytes (`is_map` ∉ {0,1} declines "map flag is not 0 or 1"), export writes the body plus Photoshop's `Crv ` v3 tail, byte-identical to Photoshop's own (532 bytes composite-only, 1560 with red and green); `.rz` conditional words 63/64 (`curves_native.rs`, `RZDC_VERSION` 64); MCP takes the maps in `params`; the Curves dialog shows a map read-only with Edit Points converting it to 16 sampled control points. `curv_map` in `adjustments_channels/` now carries and renders within 0 / 0.12 codes (8 / 16 bits) of Photoshop's own render. A new Photoshop 2025 26.10.0 take, `../core/tests/fixtures/psd_photoshop/curves_map/` (`../scripts/verify_curves_map_photoshop.py`, all five gates passed, tolerance 3 fixed before the take), authors a composite + red + green map curve: the body matches the predicted 1560 bytes, Photoshop applies the channel map first (within 0 / 0.98 codes; the other order misses by 106), and Rasterize renders it within 0 / 0.76 codes.
- `TRACKED` — `SoCo` / `brit` carriage (needs a Rasterize op first) → plan §10C "what stays open".
- `TRACKED` — measured arithmetic gaps of eight adjustment kinds (worst `expA`, then `phfl` preserve-luminosity) and seven effect kernels (glows, bevel & emboss, gradient ramp, outside-stroke corners) → plan §10C "what stays open".
- `TRACKED` — 8-bit ZIP channels, ZIP composites and `Lr32` stay refused → plan §10C.
- `DONE` — typed global light: 16-bit import sets the document's light from 1037/1049 and keeps `use_global_light`; export writes the document's light.
- `DONE` — knockout `knko`/`infx` crosses PSD at both depths (12E-3c `1063561`, measured 12E-3d `e03026e`; P7-37 verified).
- `DONE` — P7-21 closed by slice 3 (`fc70079`); disabled-effects `lfx2` fixed (`fa336cf`, `core/tests/psd_disabled_effects.rs`); `psd_zip.rs` trailing-data refusal (`fa336cf`, `psd_zip16.rs`).
- `DONE` — Photoshop 16-bit layered import (`5e0ed4d`, `psd_photoshop_depth16.rs`); omission receipts now Photoshop-authored.
- `DROPPED` — `phfl` v3 carry: Photoshop never writes it; the decline stays as unreachable (Run 2).
- `DROPPED` — contours/noise/technique/texture/pattern/jitter/glow range, `lrFX`, `lmfx` stacking, `clbl`: deliberate non-goals disclosed in `GENERAL_LIMITATIONS` (`psd_import.rs`).
