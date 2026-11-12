# Per-construct PSD exchange rules (ADR)

- **Status:** Accepted — open items remain ([#2](https://github.com/kpgalligan/doctored/issues/2), [#3](https://github.com/kpgalligan/doctored/issues/3))
- **Phase / dates:** 10C-2 slices 1–3 plus closeout Runs 1–5, 2026-09-14 → 2026-09-17 (knockout carriage added by 12E-3c, 2026-09-20); measured members and the style batches since, to 2026-10-28
- **Originals:** [archive/phase-10c-constructs-status.md](../archive/phase-10c-constructs-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-10c-constructs-status.md](../archive/2026-10/phase-10c-constructs-status.md) — this doc as of 2026-10-07, with every measurement table and issue record; [archive/phase-10c-constructs-shipped.md](../archive/phase-10c-constructs-shipped.md) — the shipped companion as of 2026-10-30, with each item's take record; [archive/2026-10-30/phase-10c-constructs-status.md](../archive/2026-10-30/phase-10c-constructs-status.md) — this doc as of 2026-10-30, before the companion fold, with its take records
- **Key commits:** `75ab10e` (slice 1), `9b6a8aa` (slice 2 phase 1), `1544fdf` (slice 2 phase 2), `fc70079` (slice 3), `502cc88` (receipts R1–R8), `693188b` / `d1b6c27` / `5e0ed4d` (Photoshop Runs 2–4), `fa336cf` (Run 5), `1063561` (12E-3c knockout)
- **Photoshop harness:** runners under `scripts/photoshop-probe/` and the `verify_*photoshop*` and style-parity drives live in the private archive `kpgalligan/doctored-dev`; takes live under `doctored-fixtures/photoshop/`.

> **Removed 2026-10-10:** smart objects, live filters and non-RAW retained sources (placed images, host
> media, Live Photo frames) are gone for v1; camera RAW layers stay. Where this record describes them, that
> code no longer exists: [removal ADR](convert-to-smart-object-status.md).

## Context
A layered PSD holds Photoshop constructs Doctored can only partly model: adjustment/fill records with no
pixels, layer effects, fill opacity, Blend If, group clipping, text/shape/smart objects. Before 10C-2 several
of these refused the whole layered import or were silently dropped. The slice fixes ONE rule per construct,
for both directions (import/export) and both depths (8-bit byte documents, 16-bit typed documents), and
requires every non-"preserve" outcome to be a sentence in the import/export report (the compatibility sheet,
and `inspect_psd` / `inspect_psd_export` for the model).

## Decisions
- **The rule table** (the contract `GENERAL_LIMITATIONS` in `../core/src/psd_import.rs` and the export preflight summarize):

  | construct | import (8 / 16-bit) | export (8 / 16-bit) |
  |---|---|---|
  | layer style (effects `lfx2`, fill opacity `iOpa`, Blend If ranges) | PRESERVE as one Doctored style on a raster, folder (Pass Through included) or adjustment record + observation at both depths (styles scope phases 1 and 2b-2, [layer styles](phase-10d-layer-styles-status.md)); an unrenderable body is DISCARDED + loss with its reason; the file's global light becomes the document's own at both depths, and carried effects keep `use_global_light` | PRESERVE as `lfx2` + `iOpa` + 40-byte ranges + observation; only what Photoshop's dialog / a PSD mask cannot hold is RASTERIZED (baked) + loss with the reason; a group whose style must bake REFUSES |
  | adjustment record the codec reads (`nvrt thrs post levl curv blnc hue2 phfl expA mixr selc` binary; `brit` with its `CgEd`; `vibA blwh` descriptor) | PRESERVE as an adjustment layer (mask, opacity, blend key, clipping) + observation; unmapped body OMIT + loss with reason | written as that record + observation |
  | Solid Color / Gradient / Pattern fill record (`SoCo`, `GdFl`, `PtFl`) | PRESERVE as a solid color / gradient / pattern fill layer + observation; a `GdFl` setting the fill cannot hold (an interpolation method other than Classic, Offset, Dither) + loss; undecodable pixel-less OMIT + loss, with pixels kept as pixels + marker loss | written as Photoshop's `SoCo` / `GdFl` / `PtFl` record (a `GdFl` pixel-less at 16 bits) |
  | any other adjustment / fill record | OMIT + loss | baked into a pixel layer + loss (`psd_export_bake.rs`); a CMYK or 32-bit document refuses it by name |
  | empty record (no pixels, no adjustment key) | canvas-sized transparent raster + observation | written as pixels |
  | text / shape / smart object; a RAW layer on export | pixels + marker loss (a smart object's contents and links are discarded) | pixels + loss |
  | group clipping | PRESERVE (`Layer::clipped`, `PrecisionDocumentLayer::clipped`) | written |
  | Blend If ranges | PRESERVE each restricted gray/R/G/B range | written as the record's 40-byte ranges |

- **Report sentences are fixed strings that tests match.** Import omission/empty/carry/decline, the typed
  global-light observation (once per file), and export preserve/bake/refuse sentences live in the importers and
  `psd_export_preflight.rs`; their exact text is in the archive. The RGB export refusal order is fixed: canvas
  side → file size → per-entry/mask side → baked style past 100 MP → group style that must bake → mask
  density/feather; a CMYK document adds an adjustment with no record and a style that must bake.
- **`GroupRecords::retains_nothing` drives both the report and the `open`/`open16` branch**, so a file of only
  omitted records reports and opens as its stored composite — the report can never advertise a layered choice the
  open refuses. A both-sides-zero rectangle is a pixel-less record (omitted or empty layer); pixel-less framing is
  exact (one stray plane byte still refuses "length mismatch"). Empty records are canvas-sized because byte painting
  never grows a layer.
- **One codec per construct family.** `../core/src/psd_adjustment.rs` is the only adjustment body parser/writer
  (both directions, both depths; `KINDS` is fourteen entries; its module doc is the byte-layout and mapping spec, with the
  `mixr` body in its private half `../core/src/psd_adjustment_mixr.rs` and `brit`/`selc` in `psd_adjustment_extra.rs`,
  measured against Photoshop-written files); `typed_description` is the only byte-form → typed adapter.
  `../core/src/psd_descriptor.rs` is framing only (version 16; bounded: nesting ≤ 16, counts ≤ 4096, every length
  checked; `obj `/`ObAr`/unknown OSTypes decline). `../core/src/psd_layer_effects.rs` + `psd_layer_effects_fx.rs` own
  `lfx2`/`iOpa`/ranges; that module doc is the complete key mapping table.
- **A new adjustment record** is written only when a Photoshop-written file in the repo or Adobe's page fixes every
  field's layout, unit and range. `brit` keeps its values in a `CgEd` descriptor of the same record (layout
  measured from `fixtures/psd_photoshop/depth16/source.psd`); `selc` follows Adobe's published 84-byte table.
  Gradient Map (`grdm`) and Color Lookup (`clrL`) fail that test in an RGB file and are baked, each with a loss
  that says PSD has the record and why it is not written.
- **Adjustment carry details.** `expA` is a BINARY body (Adobe's table), imported with `"evaluation":"document_v2"`
  so it evaluates identically at both depths; export ignores the key. `phfl` v2 colour space 7 (Lab, signed
  hundredths) is carried through `lab::lab_to_srgb8` then the file's sRGB→document transform; other spaces decline;
  export writes space 0. `curv` pads to even length. Admission charges a carried record like an empty canvas-sized
  raster. A typed adjustment carries the same blend keys a raster does (v37, [adjustment blending](phase-10d-adjustment-blending-status.md)).
- **Style carry details.** Import merges the three constructs into ONE `LayerStyle` via
  `LayerStyle::from_value(_, Strict)` and clears exactly the markers it carried (`FeatureMarkers::carry_style`); an
  identity result or a body that is not an effects descriptor leaves the marker sentence byte-for-byte. Stacked
  instances (`lmfx`) carry (up to 10 a kind; more decline); `lrFX` beside `lfx2` is the legacy duplicate
  and does not block. Style colours are authored sRGB and convert with `Transform::color` in both directions at
  both depths; a slot's wide `linear` colour crosses as `RGBC` doubles.
- **Export writes `lfx2` only when `LayerStyle::has_enabled_effects()`** (four sites plus the cost estimate; the
  import side stays on `effects.is_empty()`); `iOpa` only below 100 %; image resources 1037/1049 iff an `lfx2` is
  written (typed documents write 120°/30°). Photoshop itself surfaces no `layerEffects` for an effect-less block.
- **The bake** (`psd_export_rasterize::rasterized_style`) composites the layer into
  `style_composite::merge_extent` through the shipped compositor at opacity 1, Normal, no Blend If — those belong
  to the record's own opacity byte and blend key. An ENABLED mask is inside the bake (not written again); a disabled
  one is written as an ordinary mask and still faces the density/feather refusal. The preflight forecasts the same
  rectangle (`DrDocument::style_extent`, pixel-free) for the 100 MP and 2 GiB checks. Bake reasons: stroke < 1 px,
  satin distance outside 1–250 px, enabled mask with density or feather.
- **Photoshop-written PSD tolerances (Run 2):** the 8-bit layer-info section may carry up to three pad bytes; an
  empty-rectangle mask at outside 255 is "no mask" (at outside 0 a reported drop); the "pixels irrelevant" marker is
  read only on a record with a rectangle. Photoshop's 16-bit records in the `Lr16` tail with ZIP(-prediction) layer
  and mask planes import layered since Run 4 ([depth status](phase-10c-depth-status.md)).
- **Formats.** The importer reads PSB, ZIP and ZIP-with-prediction at 8 bits and in the stored composite (only the
  kept planes are inflated), `Lr32` (`psd_import_float.rs`), and the layer block of a big-endian unsigned 8- or
  16-bit RGB TIFF or of a 32-bit float RGB TIFF in either byte order; any other TIFF opens flat. A TIFF opened
  flat whose block holds layers (`tiff_layer_block`: any order, model, depth or BigTIFF; a readable one opened
  flattened too) gives TIFF a loss line, so File ▸ Save goes to the Save panel instead of writing over them.
- **Non-goals, fixed:** no per-export choice between preserve/rasterize/refuse; `TySh`, vector/shape and
  smart-object payloads rasterize; the glows' technique, `lrFX` and `clbl` are not interpreted (disclosed in the
  import sentence).
- **Agreement with Photoshop is measured, not promised.** Carried adjustments and written effects say "mapped onto Doctored's /
  Photoshop's own arithmetic … can differ / not qualified". Closing a measured gap means changing Doctored's own
  arithmetic or an effect kernel — a separate per-kind decision, not part of this exchange contract.
- **How a measured member ships ([#2](https://github.com/kpgalligan/doctored/issues/2)).** (1) Identify: a take
  varies one unknown at a time on binary pictures (a half plane, a disc, a corner); the runner declares the member,
  its free numbers, the fit subset, the bins and the rules in its docstring before any scoring, fits only on that
  subset, and judges on held-out take rows and every existing row on a fixed gate; a fitted rule is a hypothesis
  until a held-out or fresh take confirms it, and RECORDED is the only post-hoc edit. (2) SHIP when (i) every
  held-out row outside the named bins meets 2 / 0.5 codes and no existing row rises, or (ii) no existing row rises
  and every non-held existing row is lower; STOP with the per-row table when the member lowers the existing mean of
  row means but a minority of rows rise; KEEP otherwise. A shipped member is pinned per row on its identify JSON
  fixtures, and existing pins move lower only. (3) A member may ship per style and technique; a split is declared
  before it runs (the bevel style split is the one post-result exception). (4) Accepted divergences (owner,
  2026-10-19), named residuals and not open items: Photoshop's 35.9 px Inner Glow band at Size 32 Choke 100 %,
  which Doctored's kernel does not draw; layer-style Noise's grain moves with the layer, where Photoshop's stays on
  the canvas.
- **Gradients.** The Classic ramp is a uniform Catmull-Rom across the stops (`gradient_field::classic_color`); a
  stop's midpoint is read from the NEXT stop's `Mdpn` by the one `Grdn` reader and writer (the model, the sampler
  and the native file keep stop i → segment i). Linear and Smooth Gradient Map methods are refused for good: no
  bounded candidate fits.

## Rejected alternatives
- Refusing the whole layered import on an unreadable adjustment record — it hid valid layers; omission with a loss replaced it.
- Writing a separately enabled mask beside a baked style — it would apply twice.
- Carrying `phfl` v3 (XYZ) — Photoshop 26.10.0 never writes it; v2/space 7 was the real form.
- An effects descriptor with no enabled effect — reopens as a bare marker and Photoshop ignores it.

## Consequences
- Any new Photoshop construct must get a row in this table and a sentence, never a silent drop; new adjustment
  kinds go through `psd_adjustment.rs` only, new descriptor classes through `psd_layer_effects*.rs` only.
- Committed goldens `../core/tests/fixtures/psd_10c/*.psd` pin an unstyled document's bytes; Photoshop-authored
  fixtures under `../core/tests/fixtures/psd_photoshop/` (`adjustments`, `blend_if`, `masks`, `depth16`,
  `adjustments_channels`, `curves_map`, `styles_asl`) pin import.
- Suites: `psd_constructs.rs`, `psd_adjustments.rs`, `psd_adjustment_descriptors.rs`, `psd_layer_effects.rs`,
  `psd_disabled_effects.rs`, `psd_photoshop*.rs`; `DR_PSD_CONSTRUCT_FIXTURES=<dir>` writes the construct PSDs.
- Harnesses: `../scripts/verify_phase10_receipts.py` (in this repo); `verify_phase10_photoshop.py`,
  `verify_phase10_photoshop_appearance.py` and `verify_phase10_photoshop_depth16.py` (Photoshop 2025 26.10.0 pin)
  in the private archive.
- Receipts R1–R4 (binary and descriptor adjustment carry, `lfx2`/`iOpa`/Blend If with its bake case, the GIMP 3.2.6
  reopen of `styled_effects`) were taken 2026-09-16 in `502cc88`; none is open. GIMP attaches no filter from
  `lfx2`, so that projection is recorded, never gated.

## Open items (verified 2026-10-30)
- `OPEN` [#3](https://github.com/kpgalligan/doctored/issues/3) — Pattern Overlay at an Angle other than 90°. Angle ships at 8, 16 and 32 bits and in CMYK ink through the sheet, MCP, Actions, DRDC word 93 (`pattern_angle_native.rs`) and PSD `Angl`, rendered TWO-PASS (`pattern_tile`'s one rotation sampler; the 90° multiples exact). Photoshop's rotation at other angles is unfound: no lattice, bicubic, shear or supersampled candidate passes the gate on the takes, so TWO-PASS stays.
- `OPEN` [#2](https://github.com/kpgalligan/doctored/issues/2) — measured arithmetic gaps of adjustment kinds and effect kernels, worked by the rule above (per-run record in the #2 comments). Remaining: `expA` (worst), `phfl` preserve luminosity, `curves` (5 codes on Run 4), Vibrance (11 codes at 8 bits; no fitted candidate meets 2 / 0.5); the Inner Glow's Choke 100 % partial band pixels on anti-aliased shapes and the disc at Spread / Choke 50–80; both glows' band past S at Spread / Choke 100 %; the bevel's Chisel Hard and Chisel Soft outside Pillow Emboss (phase 2), Smooth Outer's darker outside half at 45–70°, Smooth Emboss / Pillow Emboss outside and Pillow's edge pixel; Bevel's Contour and Gloss Contour; the bevel Texture at Scale 75 % and MASK on Outer Bevel, Emboss and Pillow Emboss; the contoured Inner Shadow's Cone - Inverted interior and steep contours; the satin's contoured rows where the byte matte meets a steep contour; the inner shadow's composite inside the layer (IK1, confirmed, needs B0 over the partial ring in every compositor path); Stroke Center on anti-aliased shapes (Fill 50 %, Size 1, 50 % opacity); the gradient-glow law's one residual row (`og_g40_half30`); the gradient ramp's residuals (Diamond's off-axis extent, Radial's INT and ODD150 bins, Gradient Overlay's Diamond) and the fill clamp's unmeasured forms (32-bit linear stops, CMYK ink; the Gradient tool, Gradient Overlay, Stroke and glow gradients unbounded).
- `DROPPED` — `phfl` v3 carry: Photoshop never writes it; the decline stays as unreachable (Run 2).
- `DROPPED` — the glows' technique, `lrFX`, `clbl`: deliberate non-goals disclosed in `GENERAL_LIMITATIONS` (`psd_import.rs`). The bevel's technique, `lmfx` stacking, Pattern Overlay, pattern strokes, contours with the glows' Range, noise, the bevel's texture, and the glows' gradient and Jitter all carry.
- `DROPPED` (2026-10-30) — reopening a Doctored-written `selc` record in Photoshop: the writer follows the published 84-byte layout and is tested.

## Shipped
- **Slices 1–3 and closeout** — P7-21 closed by slice 3 (`fc70079`); disabled-effects `lfx2` fixed and `psd_zip.rs`'s trailing-data refusal (`fa336cf`, `psd_disabled_effects.rs`, `psd_zip16.rs`); Photoshop 16-bit layered import (`5e0ed4d`, `psd_photoshop_depth16.rs`); typed global light (16-bit import sets the light from 1037/1049 and keeps `use_global_light`; export writes it); knockout `knko`/`infx` at both depths (`1063561`, measured `e03026e`).
- **Formats** — 8-bit ZIP channels and ZIP composites (`22ea5a9f`, `psd_zip8.rs`); `Lr32` read and written (`psd_import_float.rs`, `psd_export_float.rs`); `brit` and `selc` carried both ways (`psd_adjustment_extra.rs`).
- **Levels and Curves** — per-channel and output Levels (`adjust_levels.rs`; `levl` records 0–3; DRDC words 61/62, `levels_native.rs`), gated against Photoshop's render at 3 codes (`psd_photoshop_channels.rs`); the per-channel stage applies before the composite one for Levels and Curves (fixture `adjustments_channels/`). The `curv` map form (`rgb_map`/`r_map`/`g_map`/`b_map`, 256 integers kept verbatim; `adjust_curves.rs`; the `Crv ` v3 tail; DRDC words 63/64, `curves_native.rs`; fixture `curves_map/`), and the dialog's Points | Pencil control, Smooth and points→map at 8/16/32 bits and CMYK ink (`CurvePencil.swift`).
- **Levels Auto** — Auto Tone / Auto Color in the Levels dialog and MCP `auto`: the whole visible backdrop below the layer counts, weighted only by the layer's own mask (or a new layer's selection). `ops_auto::tone_endpoints` keeps the measured white and the count-rule black (`adjust_auto_endpoints.rs`, `_rows.rs`, `_take8.rs`); it agrees with Photoshop's Enhance Per Channel Contrast (Image ▸ Auto Tone) within the pinned residuals, not with the default Levels-layer Auto (Enhance Brightness and Contrast).
- **Pattern Overlay** — slice 1 carries `patternFill`, the pattern table (`Patt` at 8 bits, `Pat2` at 16) and `fxrp` = offset − anchor both ways (`psd_patterns.rs`, `psd_pattern_fill.rs`); 32-bit (codes / 255 as linear, `float_pattern_overlay.rs`) and CMYK ink (`cmyk_style::effect_inks`, `cmyk_pattern_overlay.rs`); AT-CREATION: every layer carries a reference point (`Layer::reference`, DRDC word 91 + `LREF`, `reference_native.rs`, PSD `fxrp` on every record) and a linked overlay without an anchor anchors at offset − reference (`style_pattern::made_at`, `style_pattern_reference.rs`); ODD-DOUBLE tiling of an odd side (`style_pattern_overlay.rs`).
- **Reference point** — Image Size, Canvas Size, Crop, the angled crop, quarter turns, flips and Free Transform map the reference point and each linked anchor by the op's own map, rounded (`reference_point.rs`, `doc_reference.rs`); a filtered Rotate Canvas sizes the canvas ceil(bw) × ceil(bh) with the old top-left at (⌊(W′−W)/2⌋, ⌊(H′−H)/2⌋) (`doc_rotate::rotated_canvas`) and maps by SWAP; a turn or flip gives each LINKED fill a turned tile copy; Canvas Size floors an odd centred delta. Tests `style_pattern_reference_geometry.rs`, `_turns.rs`, `_rotate.rs`, `canvas_rotation.rs`; drive `../scripts/verify_reference_point.py`. `DROPPED` (2026-10-30) — measuring unlinked fills under a turn, scaled turned tiles and odd-shrink centres; an angled turned tile goes with the rotation resampling of [#3](https://github.com/kpgalligan/doctored/issues/3).
- **Stroke Fill Type** — phase 1: Shape Burst (`style_stroke_fill.rs`) and Dither (`style_gradient`, Gradient Overlay too) at every position, depth and CMYK ink, DRDC word 94 (`gradient_options_native.rs`), PSD `GrdT` `shapeburst`, `Dthr`; residual: the Center Shape Burst (Photoshop runs one ramp across the band). Phase 2: Fill Type Pattern through the one `PatternFill` (`style_pattern_fill.rs`), DRDC word 95 (`stroke_pattern_native.rs`), PSD FrFX `PntT` `Ptrn` + `Lnkd` (`style_stroke_pattern.rs`, `psd_stroke_pattern.rs`).
- **Contour** — the ten presets, a custom curve and Anti-aliased on the shadows, glows, Satin and the bevel's Gloss Contour, the bevel's Contour with Range and the glows' Range, at every depth and CMYK ink (`ContourRow.swift`, DRDC word 96 `contour_native.rs`, PSD `ShpC` via `psd_contour.rs`); kernel `style_contour.rs`, one byte LUT per effect, with the Below cap `min(C(R(v)), k R(v))`.
- **Noise** — on the shadows and glows at every depth and CMYK ink (DRDC word 97 `noise_native.rs`, PSD `Nose`); kernel `style_noise.rs`: a positional SplitMix64 grain at the layer coordinate, added where the plane is non-zero, after the contour.
- **Texture** — Bevel & Emboss's Texture (Pattern, Scale, Depth, Invert, Link with Layer, Snap to Origin) at every depth and CMYK ink (`BevelEmbossPane`, DRDC word 98 `texture_native.rs`, PSD `ebbl` keys); kernel `style_texture.rs`: the tile's Gray as height, NORMALIZED lighting while on, gain coupled to the bevel's Depth and Size; confirmed on held-out rows.
- **Gradient glows with Jitter** — Outer and Inner Glow `fill_type` gradient and `jitter` (`GlowFillRows.swift`, DRDC word 99 `glow_gradient_native.rs`, PSD `Grad` and `ShdN`); kernel `style_glow_gradient.rs` through the one `GradientSampler`; ships as a hypothesis. Recorded residuals: the glows' Jitter grain moves with the layer, where Photoshop's stays on the canvas; Photoshop reads a one-colour gradient glow back as a colour glow and draws nothing.
- **Styles panel and `.asl`** — Window ▸ Styles (`StylesPanelController.swift`): presets by group, apply/add/new/rename/move/delete, twelve built-ins (`style_presets.rs`), the library in `DR_STYLE_PRESETS_DIR` (`StylePreset.swift`), `.asl` import (`asl.rs`) and export (`asl_write.rs`, `dr_asl_encode`; Blend If as `Blnd`, one entry per restricted channel, `asl::BLND_RAMPS`), MCP `list/apply/save/rename/delete_style_preset`, `import_style_presets`, `export_style_presets`, Actions `apply_style_preset`; drive `../scripts/verify_style_presets.py`. Wide-gamut style colours (`authored_color::wide_into`, DRDC word 100 `wide_color_native.rs`); residuals: no wide colour on gradient stops, CMYK documents or LUT profiles, and an `.asl` carries sRGB only. The writer writes Photoshop's record form and `blendOptions`' full six-key form whenever a blending option is set (rule `blend-options-full`, `psd_asl_layout.rs`, negative fixture `hand/H11.asl`); a library it wrote loads in Photoshop and reads back within the declared bounds (`psd_asl_open_668.rs`); Photoshop-authored S1–S10 import as built (`psd_asl_photoshop.rs`), except a Stroke Emboss bevel, dropped and named in an `.asl` and declining the whole style in a PSD.
- **Gradients and fills** — `SoCo` and `GdFl` carried both ways at 8 and 16 bits. Measured: Linear, Angle, Reflected and Radial geometry, the Shape Burst Stroke, Smoothness (DRDC word 103), Align with Layer (105), stop midpoints (104), the editor's midpoint diamonds and the Gradient tool's Smoothness, at every depth and CMYK ink (`style_gradient_identify_parity.rs`, `style_shape_burst_identify_parity.rs`). Gradient Map `method: "classic"` with the Classic term clamped to its bracketing stops (`gradient_field::ClassicBound::Bracket`; DRDC word 111; CMYK `grdm`; `gradient_map_identify_parity.rs`; a map without `method` is the legacy linear map), and `method: "perceptual"` over Oklab (`oklab.rs`; RGB only, a CMYK document refuses it by name). The fill clamp holds a Gradient Fill's Classic term to its bracketing stops (`style_gradient::fill_bound`, `gradient_fill_bracket.rs`). Two deliberate re-read changes: a CMYK file an earlier build wrote (interpolation 4096) reopens as Classic, and an earlier PSD or `.asl` reads its midpoints one stop earlier.
- **Measured members ([#2](https://github.com/kpgalligan/doctored/issues/2))** — each with its parity test:
  - `5d046383` the outside-stroke corner by a 5×5 chamfer, and `document_v2` Exposure per channel at the profile's gamma (`style_stroke_corner_parity`, `exposure_photoshop_parity`); `529fccd7` Exposure in ROMM and ITU-709, the outside band from the anti-aliased edge.
  - `cc9f72a5` 8-bit Posterize exact and Vibrance's chroma-anchored pull; `6e911f97` Vibrance in the document's linear light (`vibrance_law.rs`); `4855e060` / `24b3c6a7` Levels' toe as Color Balance's Hermite (`adjust_color_balance::Toe`); `cb40e076` 8-bit Levels and Black & White Tint on integer paths (`adjust_bw8.rs`). Levels and Posterize were confirmed on a fresh take.
  - `4fd00c1b` the outside stroke under the layer as a disjoint union (`band_under_layer`); `77fe68d4` byte Opacity and Fill below 32 bits (`opacity_byte`) and the Outer Glow's fitted member; `7cb52abb` the soft Drop Shadow, Inner Shadow and Satin member (`style_matte`).
  - `59c5eaa9` the Inner Glow law `L′` below Choke 100 % (`style_fx_inner_glow.rs`); `4c2e6b6c` the edge Inner Glow at Choke 100 %; `6cd13bd0` the centre Inner Glow at Choke 100 % below Size 24; `1c59044e` `L′`'s band from the anti-aliased contour (`style_aa_band.rs`, `style_inner_glow_aa_parity`).
  - `60e3738c` the bevel law `B` for Smooth Inner and Outer Bevel (`style_bevel_smooth.rs`), Smooth Emboss and Pillow Emboss as `B` at half the Size (`6cd13bd0`); `bca4ffc3` the Chisel law for Chisel Hard and Chisel Soft Pillow Emboss (`style_bevel_chisel.rs`, `style_bevel_chisel_parity`); Smooth Outer Bevel's outside cut SO1 (`style_bevel_smooth::cut_outside_with_job`, `style_bevel_outside_law`).
  - `289b1976` / `cffe24b0` the Inside Stroke's band on a shape with a partial pixel by NA1 (`style_stroke_distance::inside_corner_band`, `style_stroke_aa_parity`).
  - Effect kernels: the Drop Shadow's box-pair law D1 (`style_matte::law_matte_with_job`) and its knocked-out landing under the layer DK1 (`style_composite::SHADOW_UNDER_LAYER`); the Inner Shadow's law I1 and its contour on the unrounded matte IC1 (`style_fx_inner_shadow::CONTOUR_UNROUNDED`); the Outer Glow's law O1 and composite `g (1 − a)` (`style_effect_law`); the Satin's box-pair matte and per-copy contour (`style_fx_satin.rs`, `style_satin_law`).
  - Gradients: `ef5c7ee7` Linear and Angle, `e167d95d` Reflected, `a213bc74` Radial, `358dc768` the Shape Burst Stroke, `e9774987` Smoothness and Align with Layer, `c41f76e6` stop midpoints, `ea85e58f` the midpoint diamonds, `c398fa39` the Gradient tool's Smoothness.
